# Developing for Oscairn

Oscairn hosts ordinary CLAP plugins and, when enabled at build time, LV2 plugins. You do
not need a separate Oscairn plugin format or the desktop UI to write DSP. Platform-native
instruments additionally implement concurrent sound contexts. Start with the path below
that matches your component; the [documentation index](README.md) links the full contracts.
Bringing an existing CLAP plugin over? See [Porting a plugin](plugin-porting.md) for what it
must meet, today's limits and the optional extensions.

## Initial-development compatibility policy

Oscairn is in initial development. Our own APIs, extension drafts, data structures and
saved formats have no backward-compatibility guarantee. Replace obsolete designs outright;
update callers, tests, fixtures, schemas and documentation together. Do not retain old
readers, aliases, adapters or parallel implementations solely to support earlier Oscairn
builds. Recreate development fixtures when a format changes instead of adding migrations.

Keep version/identity checks and input validation: unsupported data must fail clearly,
without corrupting the current session or overwriting the source files. Version tags
identify contracts; they do not promise support for earlier versions.

This policy does not remove support for external standards such as CLAP, LV2 and MIDI,
or the planned cross-device portability contract. **Backward compatibility becomes a
product requirement with the first public product release.** Before that release, define
the supported API/format versions, migration and deprecation rules, and regression fixtures
for released user data. Preserve released work through supported upgrades; do not apply
today's recreate-the-fixture policy to customers' saved sounds or Performances. This does
not require preserving experimental pre-release formats now.

## Choose the boundary

| Component | Interface today | Reference |
| --- | --- | --- |
| Instrument, sampler or effect | CLAP; optional LV2 hosting | [First-party plugins](../plugins/README.md), [LV2 subset](../core/lv2-host/README.md) |
| Sequencer or note processor | CLAP note output ports and transport; optional parameter-target events | [Step Sequencer](../plugins/step-sequencer/step_sequencer.cpp), [MIDI effects](../plugins/midi-effects/README.md), [RFC 0002](rfcs/0002-param-target.md) |
| Native multitimbral instrument | CLAP plus `oscairn.sound-contexts/draft-5` | [Basic Synth](../plugins/basic-synth/basic_synth.cpp), [Test Sampler](../core/engine/tests/test_sampler.cpp), [shared contexts](../core/clap-ext/include/oscairn-clap/sound-contexts-plugin.hpp), [RFC 0005](rfcs/0005-sound-contexts.md) |
| UI, editor or automation script | Typed `Control` or JSON-RPC over the local Unix socket | [Protocol and schema](control-protocol.md) |
| New host/application | C++20 `Session` or `Graph`, supplied audio buffers and events | [Core](../core/README.md), [hosts](../hosts/README.md) |
| Hardware panel/layout | Desktop JSON layouts and device profiles today; timestamped hardware services are proposed | [Layouts](../hosts/standalone/layouts/README.md), [profiles](rfcs/0024-device-profiles.md), [RFC 0004](rfcs/0004-hardware-services.md) |

VST3 hosting, plugin GUI embedding, a distributable Oscairn SDK,
automatic DSP/auxiliary-core offloading, and a native DJ deck engine
are not implemented. A plugin binary must match the host OS, CPU architecture and ABI.
A macOS plugin cannot be copied unchanged onto an ARM Linux image.

## Build a working reference

Run from the repository root, with CMake 3.21+, Ninja and a C++20 compiler:

```sh
cmake -S . -B build-dev -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build-dev
ctest --test-dir build-dev --output-on-failure
build-dev/core/clap-host/kick-clap-scan build-dev/plugins/oscairn-basic-synth.clap
build-dev/hosts/oscairn-render reference.wav --seconds 4 \
    build-dev/plugins/oscairn-step-sequencer.clap \
    build-dev/plugins/oscairn-basic-synth.clap \
    build-dev/plugins/oscairn-delay.clap
```

The tests' sampler, a small plugin built from [core/engine/tests/test_sampler.cpp](../core/engine/tests/test_sampler.cpp), is built with them; nothing is fetched for it.
The renderer writes an offline WAV; it does not open an audio device. Socket tests need
permission to bind local sockets. For live output build the [desktop/helper](../hosts/README.md).
For LV2 add `-DOSCAIRN_LV2=ON` and install Lilv/LV2 development packages as described in
the [LV2 guide](../core/lv2-host/README.md). The default build does not require JUCE, but it does require Rust (see below).
`-DOSCAIRN_MODULAR=ON` (implied by `OSCAIRN_STANDALONE`) also builds the
[Modular instrument](../plugins/modular/README.md), which downloads and links JUCE.

Every configuration needs Rust 1.95.0 through rustup (`tools/install-rust.sh`): the sample pack
library and the engine core are built with cargo. The engine core, which holds the engine thread's
note routing and voice budget and the graph's audio and run loop, is Rust (its C++ reference is gone;
see [The engine core's vectors](#the-engine-cores-vectors)). The CLAP host is Rust by default (`core/clap-host/rust`); its C++ reference
(`kick::clap::Host`) is always compiled for the comparison tests, and `-DOSCAIRN_CLAP_HOST=cpp` makes the engine and the
plugin worker use it. The plugin worker's process is C++. Other options:
`-DOSCAIRN_UI=ON` builds the Rust UI, `oscairn-ui` ([Rust UI](rust-ui.md)), with cargo;
`-DOSCAIRN_AUDIO_O3=ON` compiles the audio path at `-O3` (the device image sets it);
`-DOSCAIRN_ENGINE=ON` builds the separate engine helper, `oscairn-engine`.

### Run the tests

`ctest --test-dir build-dev` runs every test the configuration built; `-R` picks some by name
(`ctest --test-dir build-dev -R '^control-'`). A test never touches the developer's own folders:
the top-level `CMakeLists.txt` runs every registered test with `HOME` set to the empty
`build-dev/test-home` and, unless the test names its own, `OSCAIRN_DATA_HOME` and
`OSCAIRN_CONFIG_HOME` under `build-dev/test-data/<test>/`. The `test-home-clear` fixture empties both
before a run, and `test-home-empty` runs last and fails if anything appeared in `test-home`, so a
test that unsets a folder variable or reaches for `~` is caught; restore a variable's old value
instead of unsetting it. The `control-library-scale` test's full 50,000-item run, with its 50 ms search check, is for
optimized builds (RelWithDebInfo, Release); a Debug build runs the same test on 5,000 items without the timing gate. The names
group by area:

| Names | What they cover | Built with |
| --- | --- | --- |
| `kick-clap-*` | The CLAP host: loading, state, the scanner, benchmarks | default |
| `graph`, `graph-lifecycle`, `note-routing`, `audio-routing`, `param-target`, `sequencer-*`, `midi-*`, `transport-tempo`, `sound-contexts`, `effect-contexts`, `prepared-instances`, `choke-fade`, `wheels`, `pressure`, `sampler`, ... | The engine and the reference plugins | default |
| `worker-host`, `worker-failures`, `worker-recovery`, `handoff-bench*` | Plugin worker processes | default (`handoff-bench` on Unix) |
| `*-workers` | The same engine and control tests with every plugin in a worker (`OSCAIRN_PLUGIN_WORKER` set) | default |
| `control-*` | The session, control API, library, Performances, archives, signing (`control-signing`, `control-archives`), budgets (`control-plugin-failures`), physical controls and control surfaces | default |
| `render-*` | `oscairn-render` output: repeatable and independent of block size | default |
| `engine-process-*` | Starting and reconnecting to an engine process | default, on Unix |
| `desktop-*`, `midi-monitor` | The desktop app's models and screens, the MIDI monitor | `OSCAIRN_STANDALONE` (`midi-monitor` also with `OSCAIRN_ENGINE`) |
| `lv2-*`, `*-lv2`, `plugin-state` | LV2 hosting | `OSCAIRN_LV2` |
| `modular` | The Modular instrument | `OSCAIRN_MODULAR` |

`desktop-shell` also drives a third-party plugin when `OSCAIRN_THIRD_PARTY_CLAP` (a CMake cache
path) names one that exists.

#### Timing tests

Some tests read the clock: the load guard, the worker failure and extra-block tests, the pump. On a shared CI
runner (GitHub sets `CI`) or in a sanitizer build the machine stalls and throttles them, and a failure there
says nothing about the code. One helper, `core/test-support/relaxed.h` (`oscairn::test::relaxed()`, true when `CI`
is set when the test runs, or in a sanitizer build), is the only meaning of "relaxed" and every test uses it; the
`--no-timing` argument CMake passes (sanitizer builds only; `CI` is no longer read at configure time) and `relaxed()`
both turn off a test's `timing` assertions, so a build behaves the same wherever it runs. Off CI nothing is
relaxed: every assertion is exact, on a quiet machine.

The rule for an assertion that depends on wall time: where a slow machine can only add the failure, a measured
premise decides whether that run's claim is checked; the other direction stays strict. The premise is something
the test measures in the same run (lost blocks counted by the guard, blocks that took as long as the time they had,
involuntary context switches, a second session played block by block beside the first), never a longer limit and
never a retry. A run whose premise fails says so on its output and does not check that claim. Not skipped under
CI: what a slow machine cannot fake, the direction that shows the fault.

| Test | Checked only when the premise holds | Always checked |
| --- | --- | --- |
| `load-guard` | no note given up with one or two held-up blocks, and none lost by the cheap lane: the guard counted no more lost blocks than the test held up and gave no note up for the callback's own time | a sustained overload gives notes up; notes give way in both parts; the other part loses none; the worker is not restarted (off CI, with the rest under `timing`) |
| `guard-deadline` | none given up while within the deadline: no two of four blocks took as long as the time they had | notes give way when the deadline is reached; a profile's threshold gives notes up |
| `worker-failures` | blocks past the period count as overruns when the machine did not take the processor from the process during them | sound identical to playing alone; each failure named and given up; allowance reported (overruns off CI only, with `timing`) |
| `extra-block-paced` | the delayed blocks lost to the stall are counted within 10 calls of it, not across the run | stereo equal; the timing checks run off CI only |
| `control-shared-costs` | a voice costs no less shared than alone (off CI) | a voice costs no more shared than alone (the fault is three times): both sessions are played block by block together |
| `control-performance-switch-stress-hosts` | | its first Performance's workers are all given up before the next one opens, so none is a dead instance the supervisor has not seen yet |
| `plugin-groups-*` (chain kill) | another part's silent block counts against the kill when no plugin on its path (the shared instrument, its chain) missed its deadline from the block before to the one after; part 3 as loud as before when none on its path missed one around the blocks compared | the other parts never silent off CI; part 3 goes silent and comes back; every insert of its chain restarted with its own gain |
| `voice-importance-scenario` | the lead gives up nothing and keeps both keys, the piano loses nothing for the lead's own lane, no flapping over 2,000 blocks: the guard counted no lost block on the part the claim is about; the piano's claim counts only what the rules that take across lanes (the device's, the losses', the deadline's) gave up, not what its own lane gave up for its own time (a slow machine's voices cost more than the fixture's figure), and only when no lane was held off its core and neither part lost a block | no part silent; every plugin kept to its caps (`importanceMismatch` 0); the piano's voices give way where they free what is short; the lead gives up its own for its own lane |
| `worker-service` | services shown at the interval at first and after a parameter is set, and ticks past it held back once quiet: the ticks came in time to show them | each tick serviced the worker only once due, and always once due, by the clock it ran at (the last service's tick bounds its ask) |

Other tests with a `timing` flag (`guard-*` apart from `deadline`, `worker-timing`, `control-worker-deadline`,
`worker-extra-block`, `plugin-hosts`, `engine-pump`) still skip their `timing` assertions whole when relaxed, and
have not yet been given a premise of their own; `worker-wake`, `control-performance-switch` and the local-socket test
use `relaxed()` to repeat a timed scenario.

#### The engine core's vectors

`engine-core-diff`, `engine-audio-diff` and `engine-run-diff` drive the Rust engine core with random plans and event
streams (plan n from seed n; 60 plans of 3,000 blocks, 200 of 2,000 and 150 of 1,500) and compare every result bit for
bit with recorded outputs in `core/engine/tests/vectors` (`vectors.h`: a running digest of every value compared, 16
bits of it per block and all 64 every 64th block). A mismatch names the plan and the first block that differs. The
files hold the outputs of the C++ reference the core was ported from and proven against, recorded before it was
deleted; each file's header names the reference's commit (`bd57eed7`), the seeds, the sizes and the
floating-point conditions. That reference cannot be run again without checking out `271f6e9a`, the last commit with
it and its record mode. `engine-core.vec` and `engine-run.vec` were since recorded from the Rust core at `022702d9`,
whose change was the first from the reference: a plugin on a lane a block late has its cost per note measured
against the notes of the block it rendered, the block before. All three were recorded again at `c111f159`, for
[RFC 0046](rfcs/0046-voice-importance-across-parts.md), accepted by the owner: the budget takes the least important
voices among those that free the time that is short, by one importance order, in place of the lane where a voice
frees the most time, and a listing plugin's caps count the notes admitted since its report (`engine-core.vec`,
`engine-run.vec`); `engine-audio.vec` only because a node's activity gained two counters (`importance_mismatch`,
`held_off`), which its digest includes: its samples replayed unchanged against the vectors before, with the
activity's digest cut to its old size. `engine-core.vec` alone was recorded again at `7f01e5e2`, on the SDK owner's
review of RFC 0046: the host's own notes count as just struck for 50 ms plus one block (they are stamped at their
block's start, so their age runs up to a block ahead), which changes some victims; `engine-run.vec` and
`engine-audio.vec` replay unchanged. `engine-core.vec` alone again at `d6b5b6a2`: a draining connection's voices (the
last Performance ringing out) rank below every live voice of their tier, which changes some victims in plans with
draining sources; `engine-run.vec` and `engine-audio.vec` replay unchanged. `engine-core.vec` alone again at
`b6fa3f2b`: a held key whose voice its plugin ended no longer counts as a sounding note in the budget (with or without
the importance report), a listed held or pedaled voice goes for a new note only past the lane's limit, and a listing
plugin's over-limit count and its mismatch check read the voices it keeps; `engine-run.vec` and `engine-audio.vec`
replay unchanged.

The conditions record and replay share: the core is always built `cargo --release`; the tests and `vectors_math.c`
are built `-ffp-contract=off`; and the test executables define `exp`, `log1p` and `pow` themselves
(`vectors_math.c`, IEEE arithmetic alone), which the core calls in place of the platform's libm, whose last bit
differs between macOS and glibc. A test fails if the core never reached them. So the vectors do not depend on the
build type, the compiler or the libm. The generators draw each random number in its own statement (a call's
arguments and an operator's operands are evaluated in an unspecified order: GCC on x86_64 goes right to left), and
samples are digested with every NaN as one pattern (a NaN made from infinities has the CPU's sign: x86's is set).

The rule: a change to the core that changes what these tests see is a change to the core's behavior, and needs new
vectors recorded from the Rust core itself (`oscairn-core-diff-test record FILE COMMIT`, likewise the other two,
after which the header names `rust_core_` and that commit), with the reason in the commit and a review of that
reason. A change meant to keep the behavior keeps the vectors: if they fail, the change is wrong, not the vectors.

#### Per-block cost budgets

The owner's rule for the engine thread: a feature that is not acting costs nothing measurable, at rest above all.
Wall time cannot hold a gate to that (this Mac's thread times are bimodal, a CI runner's are anyone's), so two tests
count the work a block does and fail when a count passes its recorded budget, on any machine:

| Test | What it counts, per block | Budget at rest | Budget with keys held |
| --- | --- | --- | --- |
| `engine-cost-budget-core` (`core/engine/rust/src/run.rs`, `cost_budget`: eight instruments on lanes of their own that give up their own voices, the core bench's shape; held, 30 keys each and four of them listing their voices) | budget entries visited by a pass over them; note slots read (`NoteRoute::slot`); listed voices ranked; calls into the C++: `voices`, `importance`, `waited`, `set_limits`, `took` | 136, 0, 0; 16, 8, 0, 0, 8 | 232, 0, 0; 16, 8, 0, 0, 8 |
| `engine-cost-budget` (`core/engine/tests/cost_budget.cpp`: five voice-fixture instruments each through an insert, chain placement, a sixth kept warm) | blocks handed to workers; bytes copied between the engine and the shared blocks; system calls for them (a doorbell's wake, a sleep on the block's end) | 10, 15,360, 20 | 10, 19,380, 20 |

Both also hold the budget to giving nothing up while they count (a budget acting does more, on purpose), the system
calls to at most a wake and a sleep a block handed, and the instance kept warm to no work at all. The core's counters
(`core/engine/rust/src/work.rs`) are compiled in cargo's test build only (`cargo test`); the engine side's, four per
node in `WorkerTiming` (`handed`, `copied`, `wakes`, `sleeps`), only in the copies of the plugin host and the engine
the test is linked to (`oscairn_plugin_host_counted`, `oscairn_engine_counted`, built with `OSCAIRN_COST_COUNTERS`):
the engine and the worker count nothing.

A budget is raised only on purpose: the commit that raises one says which, from what to what, and why that work is
worth it per block; the budget is the constant beside the test's assertions (`REST`, `HELD_BUDGET` in `cost_budget`;
`restBudget`, `heldBudget` in `cost_budget.cpp`) and the table above, changed together. A count that went down is
lowered in the same change, so the next rise shows.
For the time a change costs, `oscairn-rest-bench` (`tools/engine-bench`; `oscairn-rest-bench WORKER VOICE_FIXTURE
[PARTS] [SECONDS] [warm]`) prints the engine thread's CPU and user time, syscalls and context switches per block at
rest; compare two builds by alternating runs, by their minimum, or count the instructions of `rust_core_run_block`
under callgrind (`valgrind --tool=callgrind --collect-atstart=no --toggle-collect=rust_core_run_block`, Linux), which
do not move between runs.

### Tools

Programs in `tools/`, and the ones built beside the tests, for plugin work:

| Program | Built as | Use |
| --- | --- | --- |
| `kick-clap-scan` | `build-dev/core/clap-host/kick-clap-scan` | List a CLAP module's plugins and why any fails |
| `oscairn-render` | `build-dev/hosts/oscairn-render` | Render a chain of plugins offline to a WAV |
| `oscairn-plugin-worker` | `build-dev/core/oscairn-plugin-worker` | The worker process that runs a plugin outside the engine |
| `oscairn-plugin-check` | `build-dev/core/control/oscairn-plugin-check` | Check a plugin through the library, an archive and the plugin store ([below](#test-before-publishing-a-plugin)) |
| `oscairn-sign` (`tools/oscairn-sign`) | `build-dev/core/control/oscairn-sign` | Make a signing key, sign a plugin build or an archive, verify an archive, sign and verify a package repository's files ([below](#signing-keys-and-plugin-builds)) |
| `oscairn-midi-monitor` (`tools/midi-monitor`) | with `OSCAIRN_STANDALONE` or `OSCAIRN_ENGINE` | Print what a MIDI controller sends, for writing its profile |
| `oscairn-engine-timing` (`tools/engine-timing`) | with `OSCAIRN_STANDALONE` or `OSCAIRN_ENGINE`; installed beside the engine | `oscairn-engine-timing SOCKET [--seconds N] [--no-reset]`: reset a running engine's timing window (`engine.timing`), wait N seconds (10), and print the device callback's duration and lateness and each worker's block round trips, with their parts (when the engine came for the block, the worker's wake-up, its run and the CPU time its audio thread used in it): p50, p99, p99.9 and the maximum, steady and startup apart, a table of the steady histograms, and the deadline misses and underruns over the interval, with each worker's blocks played late and silenced, the silenced ones not handed over because it was busy, the panics after a miss, its audio thread's page faults and involuntary context switches, and its worst block: the run beside the CPU time of that same block and when it was in the window. The app's engine listens on `engine-*.sock` in `OSCAIRN_RUNTIME_DIR` (by default `$XDG_RUNTIME_DIR/oscairn` on Linux, else `/tmp/oscairn-UID`); a device image's headless engine on `/run/oscairn/engine.sock` |
| `oscairn-handoff-bench` (`tools/worker-bench`) | `build-dev/core/oscairn-handoff-bench`, on Unix | Measure handing audio blocks to worker processes and back |

`tools/sampler-research` holds a research script, not a build tool. A device family's own tools
live with its code in `devices/`: `oscairn-panel-holder` and `oscairn-panel-dump`
(`devices/roland-fantom0/tools`, built in `build-dev/devices/roland-fantom0/` on Unix) hold a
FANTOM-0's panel device and print its messages
([running on a FANTOM-0](fantom-0.md#the-panel-holder)).

### Environment variables

| Variable | Effect |
| --- | --- |
| `OSCAIRN_PLUGIN_WORKER` | The worker program that runs plugins ([RFC 0010](rfcs/0010-plugin-worker-isolation.md)). Unset, the desktop's engine helper uses the worker beside it or where it was built, and other hosts run plugins in the engine. Set but empty, plugins run in the engine. LV2 plugins always run in the engine |
| `OSCAIRN_PLUGIN_SCAN` | The scan helper a loaded plugin's preset listing reads through ([RFC 0005](rfcs/0005-sound-contexts.md#presets)). Unset, the engine uses the one beside it or where it was built, and the desktop's in-process mode the one beside it; with none, a plugin in a worker lists no presets (saying why), and one in the engine's own process indexes there |
| `OSCAIRN_WORKER_MEMORY_MB` | A worker's memory limit when its plugins do not declare their memory; default 4096, 0 for none |
| `OSCAIRN_CGROUP` | Linux: a delegated cgroup v2 subtree. The engine moves into `engine/`, each worker gets a cgroup under `workers/` with its memory limit as `memory.max`, and memory pressure is read from the subtree. Without it, workers have no cgroups and pressure comes from `/proc/pressure/memory` |
| `OSCAIRN_RELIABILITY` | `instrument` or `pass-through`: on Linux the engine and workers lock their memory and take their places in the out-of-memory order, and say on stderr what they could not do; threads they start after that get 1 MiB stacks instead of `RLIMIT_STACK`'s 8 MiB, which locking would keep resident ([RFC 0025](rfcs/0025-resource-budgets.md), [RFC 0027](rfcs/0027-reliability-levels.md)) |
| `OSCAIRN_AUDIO_THREAD_CORES` | Linux: a CPU list (`1-3`, `2,3`). The engine's audio thread, the graph's helpers and each worker's audio thread keep to those cores (`pthread_setaffinity_np` as each starts); every other thread runs anywhere. Unset, nothing is moved. The app sets it from its profile's `compute.audioThreadCores` unless it is set already. A list the cpuset the engine runs in excludes is said on stderr and not applied |
| `OSCAIRN_CPU_LATENCY` | Linux. Unset, while its audio device runs the engine holds a PM QoS request for no CPU wake-up latency (`/dev/cpu_dma_latency` held open with a 32-bit 0 written), so idle cores stay out of deep idle states and a thread woken for a block runs at once ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#cpu-wake-up-latency)); a device node absent or not writable is said once on stderr. `keep` asks for nothing, for comparing |
| `OSCAIRN_TIME` | `known` or `unknown`: whether the wall clock is right ([RFC 0035](rfcs/0035-system-base.md#time)). Unset, it is known unless it reads before 2026-11-01. With it unknown, recordings are named `Take 0001 …` instead of by the time |
| `OSCAIRN_DATA_HOME` | The data folder: the library, `assets/`, the plugin store `Plugins/`, signing keys `Keys/`, `trust.json` and `measurements.json`. Default `~/Library/Application Support/Oscairn` on macOS, `$XDG_DATA_HOME/oscairn` or `~/.local/share/oscairn` on Linux. Point it at an empty folder to test as a fresh device |
| `OSCAIRN_CONFIG_HOME` | The desktop's configuration folder (themes and the selected theme; see [design language](design-language.md)) |
| `OSCAIRN_RUNTIME_DIR` | Where the desktop and its engine helper put their socket |
| `OSCAIRN_WORKER_BUDGET_US` | A plugin worker's allowance: how long, in microseconds, the engine waits for a block it handed the worker before the block is missed and silenced ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#the-allowance-today)). Unset, or not a whole number above 0, half the block's period (1333 at 128 frames and 48 kHz). The app sets it from its profile's `compute.workerAllowance` unless it is set already. Tests set 50000 so that worker results do not depend on the machine's load |
| `OSCAIRN_WORKER_DEADLINE` | `1`: each plugin worker waits for its block until its wave's deadline in place of its allowance ([RFC 0045 step 6](rfcs/0045-audio-scheduling-on-small-devices.md#the-allowance-from-the-deadline-step-6-status)): the audio callback's deadline (its start plus the period less 15 % for the engine's own work) less what the waves after it took in the last block. Unset: the allowance. Either way no wait on a worker goes past the callback's deadline when the engine runs an audio device. The app sets it from its profile's `compute.workerDeadline` unless it is set already |
| `OSCAIRN_WORKER_SPIN_US` | How long, in whole microseconds, an audio thread waiting on a plugin worker spins before it sleeps ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#spinning-before-sleeping)), held to half the allowance: the engine's thread for each block's result, and each worker's audio thread around its next block's expected hand-over, both only while every worker's audio thread and the engine's have a core each. Unset or 0: none, both sleep at once. The app sets it from its profile's `compute.workerSpin` unless it is set already (200 on the FANTOM-07) |
| `OSCAIRN_WORKER_REST` | Which plugin workers whose plugin is asleep with nothing for it are left uncalled, saving the round trip ([plugin porting](plugin-porting.md), "Asleep means not called"; the plugin skips its own work either way). Unset or `all` (the default since 2026-11-27): any worker, once asleep with nothing for it for 192 blocks in a row (0.5 s at 128 frames); the first block after it is a little dearer, the note's own block is always called. Workers of the extra block never rest. `disabled` (the earlier default): only one no note can reach in the Performance playing (every note connection into it off, from the input or from nodes that can be played, and no audio from the input or such a node: a part switched off by its scene or by turning off its wires), so a part that can be played is never cold; it is called again on the very block a change turns it on. `0`: none, every worker every block. Resting changes no sample: `worker-rest-diff` plays one session (notes after silence and at the hold's edge, a tail's end, parameter changes, a panic, a plugin waking itself, the transport) with each and compares the outputs. A diagnostic switch for the device, read once at the engine's start, settable in the bundle's `etc/default/oscairn-engine`. `engine.timing` gives each worker's `called`, `rested` and `asleep` |
| `OSCAIRN_LOAD_GUARD` | `off`: a Session made with it set gives no notes up for load when no CPU budget is set ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md)'s load guard; a CPU budget set still steals as RFC 0018 says). A diagnostic and test setting, not a product mode: the tests that compare audio sample for sample use it. Unset, the guard is on |
| `OSCAIRN_SHARING` | RFC 0045's sharing. Unset or `cpu` (the default since 2026-11-17, every profile): the engine forms no shared instance on its own, so each part's instrument (the Sampler's five parts in ST Final, say) gets an instance and a worker of its own, at a core's time each and the memory of a process each (about 45 MB a Sampler instance with a piano on the FANTOM-07), and a Performance saved with parts sharing is split as it opens; one that apart would not fit the memory allowance and shared would opens shared, with a notice. `memory` or `balanced`: parts of one plugin share an instance where they can, as before, the same way on every load |
| `OSCAIRN_PLUGIN_HOSTS` | RFC 0045's plugin hosts: a whole number, 0 to 64, of worker processes the plugins in workers share in real time, each holding plugins of any kind beside each other, with one audio thread and one doorbell; an instance goes, as it loads, to the host whose plugins cost least by the Session's measurements (a plugin not measured counts a tenth of the block). Hosts start as instances need them and stay for the engine's life. A crash takes down one host, which is restarted with its instances restored; a plugin a shared host's crash or hang is put down to gets a worker of its own from then on. Unset or 0: a worker per plugin instance, as before. The app sets it from its profile's `compute.pluginHosts` unless it is set already (none on the FANTOM-07: three hosts measured worse there) |
| `OSCAIRN_OWN_HOST` | Plugin IDs, separated by commas, given a worker of their own even with plugin hosts (the profile's `compute.ownHost`); an entry ending in `.*` names a publisher (`com.example.*`: every ID starting `com.example.`) |
| `OSCAIRN_WORKERS` | How plugins share worker processes (the profile's `compute.workers`, [RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#placement-by-group-2026-11-15)): `plugin`, a worker per instance; `chain`, a worker per plugin group (a part's instrument alone, its inserts together, each bus and the master their own); `pool`, the groups in `OSCAIRN_PLUGIN_HOSTS` hosts (3 if unset), never split. Unset: as `OSCAIRN_PLUGIN_HOSTS` says. `OSCAIRN_OWN_HOST` wins over each. The app sets it from its profile's `compute.workers` unless it is set already (`chain` on the FANTOM-07; no other profile sets it) |
| `OSCAIRN_ENGINE_AUDIO_CORES`, `OSCAIRN_WORKER_AUDIO_CORES` | CPU lists for the engine's audio thread and helpers, and for workers' audio threads, each in place of `OSCAIRN_AUDIO_THREAD_CORES` for its side (the profile's `compute.engineAudioCores`, `compute.workerAudioCores`); with `chain` or `pool` each worker process takes one core of the workers' list, round-robin |
| `OSCAIRN_WORKER_GROUPS` | `0`: nodes sharing a worker process each cross to it on their own. Otherwise (the default) nodes of one process whose audio flows between them render there in one round trip ([RFC 0045's plugin groups](rfcs/0045-audio-scheduling-on-small-devices.md#plugin-groups-2026-11-15)); with a worker per plugin instance nothing is grouped. Read as each plan is made |
| `OSCAIRN_WORKER_EXTRA_BLOCK` | Experimental, off by default: plugin IDs, comma-separated (`org.oscairn.va,org.oscairn.sampler`). Each instance of a listed plugin in a worker, in real time, renders each block while the engine plays the one before: it is heard exactly one block late, and has its allowance plus a block's period for each block ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#one-block-late-for-one-part)). Read as each instance loads, fixed for its life; nothing switches it automatically |
| `OSCAIRN_UI_PAINT_LOG` | `1`: the app and each app process (the Setlist, Settings) write to their logs, once a second, a line for each window that drew: its frames, the pixels painted in them and the milliseconds painting took, each a second (`oscairn: UI paint "<window>": ...`; [RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md)). On macOS the windows then paint with JUCE's software renderer, as on Linux. Unset: nothing is counted |
| `OSCAIRN_WORKER_REPLY_SECONDS` | Tests: how long a control call waits for a worker |
| `OSCAIRN_REQUIRE_SOCKET_BIND` | Tests: fail, rather than skip, when local sockets cannot be bound |

## Make a first CLAP plugin

The [reference plugins](../plugins/README.md) are working code rather than pseudocode.
Basic Synth shows an instrument on the SDK's `sound-contexts-plugin.hpp` with `oscairn.ui`
roles, pitch bend and the mod wheel; Delay shows native effect contexts, state and a `mix`
role ([what each demonstrates](plugin-porting.md#examples)).
For an in-tree native instrument, copy `plugins/basic-synth/basic_synth.cpp` into a new
plugin directory. Change its descriptor ID, name and vendor before loading it alongside
the original. Keep the lifecycle, event and sound-context handling while changing DSP.
Do not copy source without retaining its license notices; see the repository [license](../LICENSE).

Register the source in `plugins/CMakeLists.txt` using the existing helper:

```cmake
oscairn_add_plugin(my_synth my-synth my-synth/my_synth.cpp)
```

Then rebuild and replace the synth path in the renderer command with
`build-dev/plugins/my-synth.clap`. The plugin must export `clap_entry`, supply its factory
and descriptor, and return supported extensions from `get_extension`. The helper gives
it the CLAP and Oscairn extension headers and exports a `.clap` module. An out-of-tree
plugin can use its own build system and the same CLAP C ABI; it does not link the Oscairn
engine into its DSP binary. Vendored SDK declarations are in
[`core/clap-host/third_party/clap/include`](../core/clap-host/third_party/clap/include).
The pinned SDK revision is recorded alongside them in `upstream.json`.

### The CLAP extension headers

Oscairn's own extension headers and SDK helpers (`include/oscairn-clap/`, MIT) are developed in
[oscairn-clap](https://github.com/oscairn/oscairn-clap), which also holds the reference page for
each extension and the example plugins. Oscairn keeps a copy of its `include/` and `LICENSE` at
[`core/clap-ext`](../core/clap-ext), and `core/clap-ext/upstream.json` names the repository and the
commit the copy is. `tools/check-clap-ext.py` fetches that commit and fails if the copy differs from
its `include/` and `LICENSE` in any file; it runs as the `clap-ext-upstream` test, which is skipped
without the network (exit 77), and as its own CI step, where `OSCAIRN_REQUIRE_NETWORK=1` makes an
unreachable repository a failure.

To change a header:

1. Change it in oscairn-clap (with its reference page under `docs/` in the same commit), and push.
2. Copy that commit's `include/` and `LICENSE` over `core/clap-ext/` (`rsync -a --delete
   ../oscairn-clap/include/ core/clap-ext/include/` and the license), and set `commit` in
   `core/clap-ext/upstream.json` to the oscairn-clap commit.
3. Run `python3 tools/check-clap-ext.py`. Before the oscairn-clap commit is pushed, check against a
   local clone with `OSCAIRN_CLAP_EXT_REPO=../oscairn-clap`.
4. Commit the copy, the pin and the host change that needs the header together, and update
   [plugin-porting.md](plugin-porting.md) for the host's side.

Never edit `core/clap-ext` alone: the check fails until oscairn-clap has the same change.

Stable descriptor, parameter and port IDs are identities, not display names or array
indices. Give parameters truthful ranges, defaults, units/text and flags. Host-facing
parameter values use plain units, not an assumed normalized 0..1 range. Declare stepped
and read-only parameters accurately. Do not change an ID's meaning between presets.

## Audio, events and real-time behavior

- Allocate voices, buffers and queues during initialization, activation or explicit
  control-thread preparation. Do not allocate/free, wait on a mutex, access files,
  perform network I/O or log synchronously inside `process`. CLAP host services run
  through the host's main-thread service loop; LV2 background work uses Worker.
- Honor the actual frame count, sample rate, audio-port inventory and transport validity
  flags. Calls can be shorter than the negotiated maximum. Process events at their
  sample offsets; do not apply every event at block start or assume 128/512 frames.
  The rate can change while the plugin is loaded: the host deactivates it and activates it
  again at the new rate, keeping its state, so derive everything rate-dependent in `activate`.
- Support the note dialects you advertise. CLAP note IDs, ports, channels and keys have
  separate meanings; match wildcard note-offs/expressions correctly. Emit `NOTE_END`
  when a CLAP voice really finishes or is stolen, including after its release envelope.
  Repeated presses of the same key are separate notes when IDs identify them.
- Handle sustain and emergency release/reset. Poly pressure addresses a key/voice;
  channel pressure can affect all matching voices. Oscairn converts incoming MIDI notes
  and pressure at its boundaries; a plugin must still implement the expression it accepts.
- Audio ports may be mono, stereo or multichannel, with independent sidechains and outputs.
  Declare main ports where supported. The graph sums explicit routes without automatic
  normalization or limiting. A cue output must not silently become a master output.
- Keep the audio layout stable while loaded in a graph. The CLAP adapter rejects
  structural port rescans that invalidate compiled routes. A state/preset prepared for
  an instance pool must also preserve port layout, note dialects and parameter metadata.
- Keep output events bounded and valid. The shared CLAP host permits 512 note events from
  the graph per call (a few more with the block's beat and harmony events) and a separate
  bounded control queue; overflow can invoke panic.
  The graph does not promise arbitrary event counts or unlimited polyphony.

The graph gives each note destination its own IDs, tracks ownership across source routes,
and forwards note-off/expression to the owner. MIDI 1.0 plugins cannot distinguish every
same-key voice or choke just one sustained voice: channel-wide all-sound-off may be
necessary. Test those cases rather than assuming CLAP note identity survives MIDI translation.

Transport currently represents one session timeline. Sequencers follow its beat position
and flags instead of counting fixed-sized callbacks. Independent DJ-deck playheads,
scratching and time stretching need deck-specific engines; they are not supplied by that
shared timeline. The graph and multichannel master/cue routing can be reused around them.

## Sounds, state and presets

There are two switching mechanisms:

| Capability from `sounds.get` | Responsibility |
| --- | --- |
| `mode: "native"` | Plugin implements concurrent contexts and knows its own voice/tail lifetime. Basic Synth is the reference. |
| `mode: "instances"` | Host prepares separate instances of an ordinary CLAP/LV2 instrument and preserves note ownership. |
| `mode: "unsupported"` | No sound-switching interface for this node. Its ordinary processing still works. |

All new native Oscairn instruments must support multitimbral contexts: selecting a new
sound changes new notes while old notes retain their patch, expression, sustain and
release. Share immutable samples/tables when appropriate; keep patch-dependent voices
and DSP history independent. Follow the exact threading, pinning, IDs and retirement
contract in [RFC 0005](rfcs/0005-sound-contexts.md) and its
[C header](../core/clap-ext/include/oscairn-clap/sound-contexts.h). The extension is a draft,
not a standard CLAP extension. Plugins must still behave normally in hosts that do not use it.
Native Delay now implements independent effect contexts; see [the contract below](#insert-chains-and-shared-effects).

Ordinary instruments need no custom extension for the host pool. Today it supports
instruments with note input, audio output, no audio input and no note/MIDI output. It has
eight slots including protected Default slot 0. The pool preserves separate channel
selections and can prepare defaults, a saved state directory, or a preset (an LV2 preset
URI, or a CLAP preset the plugin lists through CLAP preset discovery and loads with
preset-load), then apply numeric parameter overrides. It never restores state over the outgoing sound.
Effects and event-producing instruments need other transition policies and are not pooled.

Every processed pooled instance keeps rendering until explicitly discarded. There is no
silence detector or automatic tail timeout: a quiet passage does not prove a tail is over.
`cutTail: true` permits discarding an unselected instance only after its owned notes are
gone; it can cut remaining DSP tails. `activeVoices` for a pooled MIDI plugin counts owned
keys/sustained notes, not its private DSP voices. Plan CPU/RAM for all retained instances.
Use [the protocol](control-protocol.md#prepared-sounds) for exact requests and error behavior.

For persistent plugin data, implement the format's state extension. Parameter lists alone
cannot reconstruct sample assets, modulation or arbitrary internal state. The C++
`PluginHost` state API saves a **new** directory and reports failure rather than overwriting
an existing bundle. Raw adapter restore can reset/silence that instance; graph-pooled
instruments instead prepare and select a spare. State export, preset enumeration and typed LV2 patch-property methods also use the
[control API](control-protocol.md#state-export-presets-and-patch-properties). Query capabilities
and distinguish queued property acceptance from plugin-confirmed readback.
Complete projects are still future work. Sounds, Performances and the library are in
[RFC 0021](rfcs/0021-sounds-performances-and-library.md#progress).

## Packaging and portability

Built today ([RFC 0006](rfcs/0006-packages-and-portability.md)): an exported Performance or
Sound is a [library archive](library-archive.md) (format 3) that carries every build of each
CLAP plugin it uses that the exporting machine has, each file hashed. Installing one puts the
builds in the data folder's plugin store, and a Performance's `lock` records each build's
digest ([format](performance-format.md)). A plugin that is missing, in another version, or
fails to load opens as a stand-in that keeps its state. Builds, items and whole archives can be
signed; an install refuses plugin builds that are unsigned or signed by a key the device does
not trust, until the user agrees. LV2 plugins are not packaged: they are installed on each
device. See [Porting a plugin](plugin-porting.md#packages-and-signing) for a plugin author's
side.

### Signing keys and plugin builds

```sh
build-dev/core/control/oscairn-sign key author.key
build-dev/core/control/oscairn-sign build author.key clap com.example.synth 1.2.0 macos-arm64 MySynth.clap
build-dev/core/control/oscairn-sign archive author.key Export.olib
build-dev/core/control/oscairn-sign verify Export.olib
```

`key` makes an Ed25519 key readable only by you and prints its ID (the first 16 hex digits
of the SHA-256 of the public key) and public key. `build` adds this key's signature to
`MySynth.clap.sig` beside the build, over its format, ID, version, platform and digest;
re-sign after every rebuild. `plugin-package` packs builds and their `.sig` files into a
[plugin package](library-archive.md#plugin-packages), an archive with no Sounds or
Performances, signed whole with `--key`; a bundle holding several plugins is signed for all of
them at once (`build` takes the IDs comma-separated) and packed once, each plugin named with
`--plugin <id> <name>`, as `<id>@<version>` to both where a plugin's version is not the
package's. It refuses a build loading libraries from outside the system and its
bundle ([rules](library-archive.md#plugin-packages)) unless `--allow-outside-libraries`, which
it reports. `--license`, `--license-file` and `--source <url> <commit-or-tag>` give its license,
license texts and source, which a GPL, LGPL or AGPL plugin package must have
([license and source](library-archive.md#license-and-source)). `archive` signs an archive whole; `verify` checks every signature
and hash in one and says who signed what (`--into <folder>` picks where it unpacks to check,
on the file system with room for it). The app's own keys, for signing exports, are
`Keys/<name>.key` in the data folder (`keys.make`, `keys.list`); the keys a device trusts, and
for what (`plugins`, `apps`, `content`), are its `trust.json` (`trust.list`, `trust.set`,
`trust.remove`; [protocol](control-protocol.md)).

### Publishing a package

Oscairn's package repository takes submissions as pull requests to a GitHub repository, with no
server ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#hosting-and-cost)). Sign each
plugin build and the archive as above, then follow the submission steps in the repository's
README; its template is kept here, in
[tools/package-repository/template](../tools/package-repository/template/README.md). Its CI
checks archives with `oscairn-sign verify`; `oscairn-sign file` and `verify-file` sign and check
the repository's root and index key. A device installs from the built repository as a folder
(`packages.list`, `packages.install`; [protocol](control-protocol.md)) once its root key is
trusted for `repositories`: copy each blob from the builder's cache to `blobs/<ab>/<sha256>`
beside `repository.json`.

`tools/package-repository/live-check.py BUILD-DIR` checks the live repository at
packages.oscairn.org from a user's side, with a desktop build (`OSCAIRN_STANDALONE`, for HTTPS)
and a throwaway data folder: trusting its root key and the publisher's, installing VA, FM and
the effects (FM with its asset library), and playing a factory preset of FM offline,
in the engine and in a worker, through `oscairn-installed-play`. It needs the network, so it is
not part of ctest; run it after publishing.

### Planned

The package repository is live ([RFC 0031](rfcs/0031-package-repository-and-distribution.md));
project-level dependency management is planned. The plan uses a project manifest and exact
lockfile with a project-local dependency view backed by a shared durable content store.
A download cache is separate; clearing it must not remove installed offline dependencies.
Design new engines with stable logical/format
identities, versioned architecture-independent state and complete asset enumeration.
Declare shared code/data as versioned dependencies rather than relying on a developer's
installed system libraries. Build separately for each supported OS/CPU/runtime profile;
Buildroot hardware needs its target SDK, not an arbitrary Linux binary. Native package
releases should publish artifacts and hashes per supported profile so Linux-default exports
can include hardware builds while other hosts acquire their exact destination builds later.
Imports install only relevant target artifacts; absent builds remain explicit failures,
not permission to upgrade or substitute another format. Advanced exports can include other
platforms for offline use. Keep mutable DSP state independent even when immutable assets
or library code can share storage/mappings.

A package may supply several formats for one engine, but AU, CLAP, LV2 and VST3 states and
parameter/port IDs are not interchangeable by name. A migration must be declared and tested,
including control mappings, assets, behavior and edits after transfer. Native managed
engines should share a canonical state representation across their supported builds.
Current per-plugin state export is not proof of complete project or cross-platform
portability; preserve opaque third-party state and mark unsupported transfers explicitly.

For accelerated engines, declare CPU/FPGA/DSP/sound-chip implementations as alternative
execution plans with shared state/assets and conditional dependencies. Install only the
selected plan's artifacts, including its CPU control adapter where needed. Match actual
board/service/firmware capabilities; CPU fallback is allowed only when its sound and resource
requirements fit. A hardware implementation that changes the sound needs an explicit saved
mode or separate engine. See [backend packaging](rfcs/0006-packages-and-portability.md#backend-variants-and-device-specific-dependencies)
and [hardware contracts](rfcs/0004-hardware-services.md#backend-packages-and-sound-modes).
No automatic conversion of arbitrary CPU code to hardware is implied.

Device edits must export complete current state and assets, retaining backend provenance;
declare whether conversion is reversible and invalidate derived alternate states after edits.
Separate immutable package payloads from user presets/recordings and mutable app data so
uninstall preserves musical work. Declare every runtime/asset dependency so Installed content
can explain usage and safely remove orphaned packages. See the planned
[device round trip](rfcs/0006-packages-and-portability.md#editing-on-one-instrument-and-transferring-to-another)
and [content management](rfcs/0006-packages-and-portability.md#installed-content-and-removal).

## Native versus external; other DAWs (planned)

"Oscairn-native" means meeting the package/state/asset portability contract for advertised
targets, not being first-party or merely exporting CLAP. Third-party authors can implement
it. External plugins require a compatible destination installation or tested conversion;
unavailable nodes retain state/routes and are disabled with a visible reason. Sound-context
`mode: "native"` is a narrower implemented capability, not the future portability badge.

Native engines must also function outside the Oscairn app. The desktop delivery target is
ordinary CLAP/VST3 for supported Windows/macOS/Linux profiles and AU for macOS, using the
same engine and canonical state. Use standard notes/audio/parameters/state and explicit
capability checks when Oscairn host extensions are absent. Provide package-backed assets
and a usable editor without requiring Oscairn's control socket. A wrapper does not make
unsupported MIDI routing or per-note features work in every host.

Individual plugins, the whole Oscairn graph as a DAW plugin, and standalone use are separate
delivery paths. Prefer evaluating existing format wrappers to duplicating DSP. Keep exact
runtime/state compatibility for old DAW projects: a standard host's format ID does not
identify an Oscairn package version. Publish native formats through the package manager,
but do not claim that another DAW's entire project becomes a complete offline bundle.
See [the delivery contract](rfcs/0006-packages-and-portability.md#native-plugins-in-daws-and-standalone-applications).
The current references are CLAP-only; these additional delivery paths remain planned.

## Controllers, hosts and hardware

A plugin's render callback should not call the JSON control API. Audio and performance
events use the graph; UI commands use `Control`. Query `session.hello` capabilities, then
metadata and IDs; stage graph edits and publish them with `graph.commit`. Handle rejected
requests and queue pressure. `accepted` means queued, not rendered or audible. The
[protocol](control-protocol.md) specifies socket framing, subscriptions, numeric units,
errors, resource limits and routing. There is no WebSocket or gRPC endpoint today.

A custom C++ host owns its device and threads. Construct/control/service `Session` on
one control thread, call `process` on one audio thread, and stop processing before
teardown. The core does not open ALSA/JACK/CoreAudio itself. Current live hosts use JUCE's
device layer; Buildroot uses the Linux host. See [host configuration](../hosts/README.md)
for rate, buffer and channel mapping and [OS instructions](../os/README.md) for the image.

Keep physical-control state (absolute position, relative encoder deltas, pickup/soft
takeover, bank ownership) in the hardware/control-mapping layer, not in every synth.
Plugins expose musical parameters; designs needing raw positions must request that
separate control source. This distinction is documented in the [architecture](architecture.md)
and hardware RFC. External MIDI controllers are mapped through controller profiles, banks and
Mackie Control ([RFC 0024](rfcs/0024-device-profiles.md#controllers),
[RFC 0011](rfcs/0011-performance-ui-and-editors.md#step-6-status)); a plugin's `oscairn.ui`
roles and priorities decide which parameters those knobs reach. The hardware-services and
acceleration APIs are not implemented yet.
No plugin currently gets automatic work on a DSP or auxiliary microcontroller core.

## Test before publishing a plugin

Start with the scanner and renderer above. Add a fixture or reference-audio test appropriate
to the component, using the existing CMake test conventions. Useful executable examples:

- [`sound_contexts.cpp`](../core/engine/tests/sound_contexts.cpp): native held-note switching,
  exact-sample expression, channel parts, sustain and concurrency.
- [`prepared_instances.cpp`](../core/engine/tests/prepared_instances.cpp): ordinary CLAP/LV2
  switching, state/presets, FIFO ownership, release tails and allocation-free rendering.
- [`audio_routing.cpp`](../core/engine/tests/audio_routing.cpp): mono/stereo/multichannel ports,
  independent buses, sidechains and route validation.
- [`sequencer_timing.cpp`](../core/engine/tests/sequencer_timing.cpp): event timing across
  uneven block sizes.
- [Production LV2 checks](../core/lv2-host/tests/production/README.md): real instruments,
  effects, state assets and cross-platform measurements.

Two checks run on a plugin as built, CLAP or LV2, without writing tests:

- `build-dev/core/control/oscairn-plugin-check PLUGIN ID synth|effect [--param ID=VALUE]`
  plays the plugin in a part, saves it to the library and opens it again with every parameter
  as saved, exports it in an archive with its build, and installs that on a fresh device (a
  new `OSCAIRN_DATA_HOME`), where it must open from the plugin store and play the same. A note
  effect must send notes on instead of making sound. An LV2 plugin stops after the export. `--juce-state NAME=VALUE` sets an attribute in a JUCE
  plugin's saved state; `--add-build PLATFORM=PATH --keep-archive FILE` carries another
  platform's build, and `--install FILE synth|effect` on that platform checks it there.
- `core/lv2-host/tests/production/run.py PROBE OUT --case NAME,PATH,ID,synth|effect,PARAM,VALUE`
  renders at two rates, two block sizes, offline and realtime, repeats with a fresh instance
  and checks that a parameter edit changes the sound. Its probe is built only with
  `-DOSCAIRN_LV2=ON`.

Run both again with `OSCAIRN_PLUGIN_WORKER=build-dev/core/oscairn-plugin-worker` to test the
plugin in a worker process, as the desktop app runs it.

Exercise 37- and 512-frame blocks, multiple sample rates, simultaneous instances, repeated
keys, sustain, malformed inputs and bounded-queue failures. Test cleanup after failed
preparation and concurrent control/rendering under ThreadSanitizer. Use ASan/UBSan for
memory errors; a clean offline render is not a target-hardware latency measurement.
The host's [benchmark tools](../core/clap-host/README.md) can measure plugin workloads.

Scanning has process crash isolation. Playback runs plugins inside the engine process unless
the placement policy puts them in worker processes ([RFC 0010](rfcs/0010-plugin-worker-isolation.md)),
where a crash, hang or overrun silences only that plugin, which is restarted. The desktop app
runs every CLAP plugin in a worker; `oscairn-render` and the desktop's `--in-process` mode run
them in the engine unless `OSCAIRN_PLUGIN_WORKER` names the worker. LV2 plugins always run in
the engine.
Workers are not a sandbox: a plugin keeps the user's file and network access. In the engine,
a native plugin memory fault can kill it. Do not describe the scanner
or render error checks as a security sandbox. Public APIs are still evolving; pin the
Oscairn revision you build/test against and state your supported platforms and limits.

## Discovery, generic editors and appearance

`oscairn::scanPlugins` (`oscairn/discovery.h`) scans CLAP and optional LV2 bundles through
`oscairn-plugin-scan`, using the existing bounded worker protocol, per-bundle timeout and
cancellation. The UI never loads libraries to inspect metadata. A descriptor's format plus
CLAP ID/LV2 URI is its identity; the path locates the installed binary. Discovery is not an
engine-compatibility guarantee. Loaded plugins execute inside the audio engine, or in a
supervised worker process when the [placement policy](rfcs/0010-plugin-worker-isolation.md)
puts them there.

Expose complete parameter names, ranges, stepped/read-only flags, text formatting and
presets to make the generated editor useful. `Control::replace` / `graph.replace` prepares
a candidate and optional preset before publishing one replacement graph plan. The old
node ID/name remains, and its wires move to the new plugin's ports in the same roles; wires
that no longer fit are kept, marked unconnected, rather than the replacement being refused
([RFC 0011](rfcs/0011-performance-ui-and-editors.md#step-1-status)). A plugin that fails to
load or prepare leaves the old graph playing.
Replacement cuts that slot's old notes/tails and prepared sounds. Use prepared sound
contexts for seamless switching within an engine instead.

System UI components use the shared semantic palette and look-and-feel; device layouts
contain geometry and bindings, not private color schemes. Follow the
[design language and theme schema](design-language.md) when adding a screen. Generated
plugin controls inherit the active theme. Vendor-native GUIs do not; declarative custom
plugin pages and managed theme packages are future work.

### Insert chains and shared effects

The graph can connect an instrument through several effect nodes, fan its output into
parallel chains, and sum several instruments into one shared effect. Per-route gain can
serve as a send amount; route the dry and wet paths deliberately. These capabilities exist
in typed/JSON controls; the initial desktop browser exposes only the demo instrument and
effect slots, not a general rack/routing editor.

A shared effect instance processes the summed input with one setting/state. Two effect
nodes with different settings need independent processing state, even if both receive the same
instrument. Each needs its own mutable DSP state (delay/reverb buffers, envelopes, filter
history); executable code and explicitly shared immutable resources may be shared.
Native Delay implements [effect-contexts.h](../core/clap-ext/include/oscairn-clap/effect-contexts.h).
Create a sibling using `Control::addEffectContext(name, source)` / `effects.create`, then
use ordinary graph routes, parameters and state saving. Each sibling is a distinct graph
node and CLAP interface sharing native resources. Its engine/module survives removal of
the originating node. Other plugins continue using separate instances unless they provide
this extension. Explicit creation fails for unsupported plugins rather than claiming sharing.

Basic Synth's Output bus parameter (ID 8) selects Main or bus 1–8 for new notes. Pass it
in a prepared sound patch and route that port to its effect chain. Held voices retain
that bus, and voices routed separately are absent from Main. Buses are persistent routing
destinations; multiple sounds assigned the same bus intentionally share its chain.
Keep old chains routed until their voices and effects finish. `effects.resources` reports
Delay tail activity; removing a node deliberately cuts its tail. Automatic chain management
remains future work. Complete Performance persistence is described in the
[versioned JSON format](performance-format.md). [RFC 0007](rfcs/0007-effect-contexts.md)
records measured costs and validation; sharing Delay saves essentially no history RAM.

## Saving a Performance

Use the typed `savePerformance(directory)` / `loadPerformance(directory)` methods or
`performance.save` / `performance.load` RPCs. The [Performance format and schema](performance-format.md)
are the shared save/open/import/export contract. Native engines must snapshot all prepared
contexts; ordinary hosted instruments are saved instance by instance. The app's Performance
menu uses a control worker, so disk I/O and plugin preparation stay outside the UI/audio paths.
Keep this format, the schema, protocol and round-trip tests in sync when adding persistent data.
