# RFC 0004: Hardware services, event timing and engine backends

**Status:** proposed; architecture direction agreed. Step 1 built 2026-11-09
([status](#step-1-status)); the interfaces it drafts are not frozen

## Progress

Checked when built and tested; the sections below say how.

- [x] 1. Capability, clock and event contracts with CPU-only and fake-clock services ([status](#step-1-status))
- [ ] 2. Desktop I/O through those contracts, with latency measured
- [ ] 3. One board's timing backend, measured with loopback
- [ ] 4. Backend discovery, reservations and one reference engine
- [ ] 5. Accelerator failure and recovery, with measured benefit

## Step 1 status

Built 2026-11-09, in the core library `oscairn_hardware` (`core/hardware`), without JUCE or board
drivers. Tests: `hardware-timeline`, `hardware-service`, `hardware-plugin`, `desktop-profile`,
`image-profile-consistency` and `control-json-protocol`.

**Capabilities as data.** A device profile's `hardware` section (RFC 0024,
[schema](../profile.schema.json)) lists its `accelerators` and its device `clocks`; absent, the
device is CPU-only, as every built-in profile is. An accelerator record says what it actually
offers, not a chip label: `kind` (`dsp`, `fpga`, `sound-chip`, `realtime-core`, or `simulated` for
tests), `abi`, the service ABI its backends are written for (`<family>/<version>`), `device` (the
chip or board and revision, informative), the installed `firmware` it needs by `id` and `version`,
its `operations` (each `<family>.<name>/<version>`), resident `banks`, exclusive `regions`,
`voices`, `parts`, `memory` in bytes, its `clock`, `pipelineFrames` (0: results in the same block)
and `offline`. A clock record has its nominal `rate` in ticks per second, `driftPpm`,
`leadMicros` (how far ahead an event must reach it), `queue` and `cancel`. Each record says
whether its values are `documented` or `measured` (`evidence`); a capacity left out is unknown,
and a reservation asking for any of an unknown capacity is refused rather than promised. The core
parser (`oscairn::hardware::fromProfile`) is the only one: the app's profile reader calls it.

**The clock and event contract** (`oscairn/timeline.h`). The timeline is the audio sample counter
in an epoch. A device clock is mapped onto it by a least-squares line through its last 32 paired
readings (both clocks read together, at block boundaries), reporting its drift from nominal and its
uncertainty (the largest residual, in samples); a reading off the line by more than 64 samples is a
jump, and the fit restarts from it in a new generation. Inputs keep their device timestamp and its
origin (a capture time, or a receive time, never presented as a capture time) to graph ingress and
are placed in a block after an explicit delay, or reported late by so many samples. Scheduled
outputs carry endpoint, deadline, epoch and cancellation generation in a bounded queue (256 events)
that keeps 16 slots for note and gate releases, refuses events that cannot reach the device by its
lead time (a late release is sent as soon as it can be, never dropped), cancels from a boundary no
earlier than now plus the lead time (a note already sounding keeps its release, moved to the
boundary), and, when the link is lost, drops what is queued and hands over a release for every note
and gate the device holds; events of the old epoch are then refused. `FakeClock` is the fake-clock
service: a device clock as a function of the audio sample, with an offset, a drift and a jump.
Measured by `hardware-timeline` on a 48 MHz fake timer against 48 kHz audio, blocks of 128: a +50
ppm drift with an offset of 123456789 ticks estimated at 50.0095 ppm, mapped within 0.005 samples
(uncertainty reported 0.0004); -120 ppm with a negative offset at -120.0141 ppm, within 0.007; a
240-sample jump found once, and mapped again within 0.007 samples after it. Late input, equal-time
ordering, lead time, overflow, cancellation, the lost link's releases and the reconnect's new epoch
and clock generation are checked there too.

**The hardware service** (`oscairn/hardware.h`). `Service` mediates every plugin's access to the
accelerators: discovery and capability query; reservations counted across every plugin (voices,
parts, memory, an exclusive region held by one lease, unknown capacity refused, an accelerator
without its service in the build or without its firmware listed unavailable with the reason);
submit and collect of jobs, tied to the plugin's timeline and activation (a job from before a
reactivation is stale); and failures, a whole accelerator failing together (its failure domain),
each recorded with its time and whether the lease had a CPU fallback. A lease may reserve the CPU
time of the plugin's CPU path as its prepared fallback, from a fixed allowance until RFC 0025's
budget provides it (step 4). Reservations happen on the main thread; submit and collect neither
allocate nor block, using transfer buffers allocated at reservation. Each accelerator's board
adapter is a `Driver` for its service ABI. The CPU-only service, with none, is the process's until
another is set. The simulated accelerator (`core/hardware/tests/simulated.cpp`, service ABI
`oscairn.sim/1`, one operation rendering a sine) is a `Driver` built only for the tests.

**Plugins reach it** through the draft CLAP extension `oscairn.hardware/draft`
(`core/clap-ext/include/oscairn-clap/hardware.h`, MIT): the plugin lists the accelerators, picks
one whose service ABI it has a backend for, reserves before it activates, submits a job for the
block it is processing and collects the result, and on a late job renders that block on its CPU
path; on a failure it continues on its CPU path when its fallback was reserved, and mutes when not
while the host reports the interruption. Offline it uses its CPU path. Every Oscairn host offers
the extension; without accelerators it lists none. Plugins in worker processes (RFC 0010) list none
for now. The control API reports the service read-only: `hardware.capabilities`
([control protocol](../control-protocol.md)).

`hardware-plugin` renders a test plugin (`hardware_fixture.cpp`) for 400 blocks of 128 frames:
with the simulated accelerator declared, every block on the accelerator (400 jobs) and identical
to its CPU path (largest difference 0; the stated tolerance is 1e-6); a second instance, refused
the busy region, on its CPU path, identical; the accelerator declared but its service not built,
the CPU path with the reason in the status; a failure at block 200 with 20 us of CPU fallback
reserved, the CPU path from that block, identical to the CPU render with no gap, the failure
reported with its time; the same failure without a fallback, silence from that block and the
interruption reported; two late jobs rendered on the CPU without a gap or a failure; offline, the
CPU path, identical, with no jobs; and unloading the plugin releases its lease.

**Compiled out.** The contracts and the CPU-only service are in every build. Each accelerator's
service will be a `Driver` under a CMake and Buildroot option of its own, on only for a product
whose profile declares that accelerator; `os/scripts/check-image-profile.py` gets a feature for each
as it lands. The simulated accelerator is test code only: the check fails an image whose programs
name its service ABI, or a product profile that declares one.

Not built: device I/O through these contracts (step 2), any board's timing backend or real
accelerator (steps 3 to 5), the engine handing the profile's records to the service (the app
validates them; the engine's service is CPU-only until a real service exists), pipelined backends
(`pipelineFrames` above 0: defined, never exercised), a driver interface split into start and
finish for asynchronous transfers, recovery after a failure (the accelerator stays failed and the
plugin on its CPU path or muted until it is activated again), and the package resolver
([RFC 0006](0006-packages-and-portability.md#backend-variants-and-device-specific-dependencies)
defines how a plan names the backend and capabilities it needs, in these records' terms).

## Problem

Oscairn must run on CPU-only computers and instruments, and on devices with auxiliary
real-time cores, panel microcontrollers, dedicated DSPs or FPGAs. Engines should use those
resources where a compatible implementation improves measured performance. A saved Sound
must remain independent of the processor that happens to execute it.

There are two distinct needs: predictable physical I/O timing, and audio algorithm
acceleration. A processor may support either, both or neither. Its presence does not mean
its firmware is available, its peripherals are accessible, or arbitrary plugins can run
on it. Board wiring, firmware, memory and driver support determine usable capabilities.

Today the graph processes nodes sequentially on the calling audio thread. The desktop
host uses JUCE audio/MIDI I/O and places incoming notes and pressure at the next block's
start. There is no Oscairn auxiliary-core firmware, accelerator backend, hardware clock
mapping or scheduled external-output implementation. The QEMU image does not validate
any of these on a physical board.

## Decision

### Responsibilities

| Component | Owns |
| --- | --- |
| Plugins | Musical decisions, DSP algorithms, parameters, portable state and explicitly supported execution backends |
| Host resource manager | Capability matching, admission, reservations, preparation, backend selection and recovery |
| Hardware services | Device discovery, clock mapping, audio and event I/O, peripheral ownership, firmware lifecycle and mediated accelerator access |
| Board adapter and auxiliary firmware | Actual drivers, timers, DMA, peripheral scheduling and device-specific data transfer |
| UI / control API | Configuration, user policy and status; outside the played-event and audio paths |

Keep the core a library without JUCE or board drivers. The host supplies the services; a
desktop, Linux instrument, DAW plugin and deterministic test host can supply different
implementations. Device services may run in a separate privileged process, but their
audio-facing interface must have bounded execution and explicit transfer latency.

The full host still requires a suitable application CPU and operating environment. An
engine compiled into an MCU-only or DSP-only product is a separate reduced port, not proof
that the complete host, dynamic plugin ecosystem and UI can run there.

### Discover capabilities, not just processors

Expose versioned capability records for:

- **Compute:** backend family and ABI, supported engine kernels/firmware, sample rates,
  block constraints, numeric formats, local/shared memory and transfer requirements.
- **Timing:** input timestamp resolution, output scheduling support, clock domains,
  queue capacity, minimum scheduling lead time, cancellation support and measured jitter.
- **I/O:** audio channels, MIDI ports, CV ranges/calibration, gate/clock endpoints, panel
  controls and which service owns each peripheral.
- **Availability:** resource capacity, reservations, firmware/driver versions and health.

Capabilities distinguish documented limits from measured performance and unknown values.
An unavailable firmware image or inaccessible peripheral is reported as unavailable, even
if the silicon could support it. Timing firmware and audio kernels reserve shared CPU,
memory, DMA and bus resources together so audio work cannot starve scheduled I/O.

An auxiliary core running bare-metal or RTOS firmware is different from a Linux CPU core
reserved for audio. CPU affinity, realtime scheduling and parallel graph processing are
host policies; they do not create a DSP backend or allow arbitrary code on an MCU.

### Engines provide compatible backends

Every official portable engine provides a CPU backend. It may additionally supply DSP,
FPGA or real-time-core implementations of the entire engine or selected algorithms. Its
host-facing plugin remains CLAP (or another supported format); acceleration is beneath
that interface. Existing plugins without this capability continue on the CPU.

A backend declares its identity/version, required service ABI and kernel/firmware,
supported sample rates and block sizes, quality modes, state schema, voice/resource
limits, processing/transfer latency and whether it can render offline. The plugin owns
algorithm translation; the host does not compile arbitrary CPU plugin code for a DSP.

Backends for the same portable engine use the same musical parameters, engine identity
and portable state. Device handles, physical addresses and FPGA registers are runtime
details, never saved patch requirements. Backends must pass sound/state compatibility
tests with defined tolerances; they need not produce identical floating-point bits.
An audibly different algorithm is a distinct saved mode or engine, not a transparent
substitute. Hardware-specific third-party engines can declare mandatory hardware; the
host preserves their state and reports missing requirements on incompatible devices.

### Backend packages and sound modes

Separate the logical engine from its executable implementations. A release describes common
state/metadata/assets and alternative backend artifacts. The package resolver selects a
supported execution plan and installs only that plan's transitive dependencies; alternative
backends are not unconditional dependencies. The artifact graph and export/import rules live
in [RFC 0006](0006-packages-and-portability.md#backend-variants-and-device-specific-dependencies).

For example, one hypothetical engine release can supply:

| Backend | Device must provide | Installed engine-specific payload |
| --- | --- | --- |
| CPU | Matching OS/CPU/runtime and sufficient measured capacity | CPU DSP build and required data |
| FPGA | Compatible board, FPGA region/static shell, service ABI and capacity | CPU control adapter plus matching bitstream/kernel and required data |
| Dedicated sound chip | Accessible service implementing the required synthesis operations, parts/voices and asset banks | CPU control adapter plus mappings/data and any required loadable firmware |

A hardware backend often still needs CPU code for state translation, event submission,
buffers and host interaction. That does not require installing the engine's full CPU DSP
implementation. Official portable engines publish a CPU reference implementation, but a
hardware device need not install it unless it is selected, needed for local offline render,
or deliberately prepared as a fallback. Having a CPU fallback file installed is not proof
that it can keep up with the requested performance.

A dedicated synthesis chip is not necessarily a programmable DSP. Its backend may only send
commands to existing fixed-function synthesis and resident firmware/ROM. Capability records
must identify the actual operations, firmware and banks, not merely a chip label. Do not
redistribute resident firmware/ROM as if it were an engine dependency; declare required
installed identities and validate access. If audio output cannot be captured/routed through
the host with known latency, it cannot masquerade as an ordinary in-graph audio backend.
External MIDI control plus physical audio return is a different integration with explicit I/O.

The MODX/SWX100-style example is illustrative, not an available Oscairn target. Yamaha's
[MODX documentation](https://usa.yamaha.com/products/music_production/synthesizers/modx/features.html)
describes AWM2/FM-X engines; this establishes neither a third-party kernel API nor access
for Oscairn. Chip model/revision, control interface, firmware availability, audio path and
state/voice mapping must be investigated on actual hardware before declaring a backend.

FPGA artifacts likewise identify exact supported device/board, region, shell/interface and
clock/memory constraints. A generic "has FPGA" flag is insufficient. Full-fabric images and
partial-region images declare different exclusivity/reconfiguration rules. The Linux
[FPGA framework](https://cdn.kernel.org/doc/html/latest/driver-api/fpga/intro.html) separates
manager, bridge and region responsibilities; it does not make arbitrary bitstreams compatible
or define Oscairn's synthesis/resource contract.

Distinguish these cases when saving and selecting a sound:

- **Equivalent implementation:** same musical behavior and saved quality, tested within
  stated tolerances. Auto-select CPU or hardware according to measured resource/latency
  policy without rewriting the patch's musical requirements.
- **Different or enhanced sound:** higher-quality processing, a different filter/oscillator
  model or chip-specific character is an explicit saved mode or separate engine. If the CPU
  also implements that mode, it can restore it when resources permit, including offline.
  Do not replace it with a cheaper mode merely because that is all the destination can run.
- **Hardware-required mode:** no equivalent software implementation is available. Mark that
  Sound as hardware-required, even if other modes of the same engine are portable. Preserve
  it with an unavailable warning elsewhere; a user-approved alternate mode is a separate
  saved choice. The native portability badge applies only to its validated portable scope.

Hardware is not inherently better-sounding: the implemented algorithm and quality mode decide
that. Resource profiles may allow the same mode to run on a powerful computer but require
acceleration on a small instrument. If an engine offers mixed CPU/hardware execution, it
must declare supported plans and own the split/state/event semantics. The host must not
arbitrarily divide its voices or kernels between incompatible implementations.

### Select and reserve before activation

1. Match the Sound's engine/mode, quality, resource and latency requirements to backend
   plans, exact device capabilities and usable installed services. Package availability
   and live resource admission are separate: files can be complete while hardware is busy.
2. Estimate the complete cost, including CPU control overhead, transfers, buffering,
   accelerator regions/parts/voices, memory, other active engines and transition overlap.
   Reserve across the whole performance: two individually compatible plugins can still
   require the same exclusive FPGA region or sound-chip part. Report uncertain estimates
   rather than promising fit.
3. Apply device/session policy: automatic compatible selection, CPU preference, or a
   specific backend for diagnostics. Automatic selection uses measured profiles and
   preserves the requested sound and quality; the choice is visible to the user.
4. Reserve resources, load approved firmware/kernels, allocate buffers, prepare state and
   validate readiness outside the audio callback. Failed preparation releases reservations
   and leaves the currently playing graph intact.
5. Activate at an agreed audio boundary. Keep the selection stable while playing unless
   an explicit, prepared transition supports a change.

CPU fallback is allowed only when the CPU can sustain the requested configuration. If no
backend fits, report the reason and offer an explicit change in quality or polyphony, or
refuse activation. Do not silently reduce either. Device policy is separate from portable
Sound data; Sounds save musical requirements and explicit quality choices.

### Realtime execution and failure

How hard each product tries, and what happens when the engine itself fails (a watchdog, and
audio passing through on pedals and mixers), is set by its reliability level
([RFC 0027](0027-reliability-levels.md)); what a product's hardware is, by its device profile
([RFC 0024](0024-device-profiles.md)).

Preallocate buffers and use bounded queues. No firmware loading, device discovery,
allocation or unbounded driver/IPC waits occur in the audio callback. A hardware backend
must state whether results are available in the current processing interval or require a
fixed pipeline delay. Count DMA, cache maintenance, copies, IPC and buffering in the
deadline and latency budget. A fast DSP kernel alone is not evidence of a faster path.

Any added delay must be reported and aligned with other graph paths. Audio, notes and
parameter changes keep their intended relationship through the pipeline; same-sample
parameter locks must remain attached to the samples they affect. Reject unsupported
latency arrangements until the graph can compensate them; it has no complete latency
compensation implementation today.

On a missed deadline, ignore late results and use a defined mute/bypass policy. Recovery
runs outside audio processing. Switching to CPU without interruption requires a prepared
CPU context, sufficient reserved capacity and a compatible state/DSP-history transition;
otherwise report the interruption. Never promise that a parameter-state reload recreates
held voices or effect tails. A shared accelerator can fail several nodes together; track
that failure domain and avoid resetting hardware still serving healthy nodes.

Plugins use mediated handles and validated buffers, not unrestricted registers or device
memory. Resource leases, firmware ownership and revocation belong to the host. Loading a
kernel or FPGA image must not overwrite another engine's resources. Define supported
coexistence and reconfiguration boundaries before admitting such a backend.

### One event timeline, explicit device clocks

Use an absolute audio sample counter and a timeline epoch for event scheduling. The epoch
changes when the device/session timeline is reset, such as an audio-device restart.
Musical beat position and loop/seek position are separate from this monotonic timeline.
Each hardware clock is mapped to it using offset and rate estimates, with drift and
uncertainty reported. Do not assume two nominally identical clock rates stay aligned.

Inputs carry source identity, sequence information and their capture timestamp, not just
the time Linux received the packet. Preserve the timestamp to graph ingress, translating
it into a block offset under an explicit latency policy. Late events cannot change audio
already played: apply or discard them according to event type and report lateness.
Where a device supplies no capture timestamp, record the earliest available receive time
and identify its origin and uncertainty; never present it as a hardware capture time.

Outputs carry endpoint, payload, target presentation time, epoch, cancellation generation
and deterministic ordering for equal deadlines. Endpoint semantics specify what the
deadline means: a gate edge, calibrated CV validity, or a MIDI byte/message boundary.
Account for MIDI serialization, DAC settling and device buffering when relating external
events to audio. Do not promise simultaneous MIDI messages on one serial wire.

### Scheduling lead time and musical changes

A real-time processor executes events queued ahead of their deadlines. Sending an untimed
message through Linux/RPMsg at the desired instant does not eliminate Linux scheduling
jitter. The service advertises how much lead time it needs and how far ahead it can queue.

Ordinary plugins emit events for the block currently being processed, not arbitrary future
blocks. Initially use a bounded presentation delay, budgeted with audio output buffering,
so current-block events can reach hardware before their deadlines. Align internal audio
and external outputs against that presentation timeline. Report the resulting end-to-end
latency and reject a configuration that cannot meet its requested timing budget.
Do not run a stateful sequencer twice to invent lookahead. A future optional prediction
interface would need explicit state and edit semantics; it is not part of the first version.

Live input uses the same clock mapping but cannot be predicted. Measure its input-to-audio
latency separately from scheduled sequence jitter. Keep queues only as deep as justified
by measurements, since longer lookahead makes edits harder to apply promptly.

Stop, seek, tempo changes and pattern edits invalidate affected queued events using a
cancellation generation. The device reports the boundary after which cancellation can
take effect; already transmitted events cannot be recalled. Keep note ownership so
canceled future note-offs are replaced where needed, without cutting unrelated live notes.
Reschedule from a known boundary, applying clock/transport changes consistently to audio
and external outputs. This protocol must not wait for an acknowledgement on the audio
thread.

Define queue overflow, stale-epoch, disconnect and watchdog behavior. Reserve a path for
stop and note/gate release; report dropped or late events and queue occupancy. Loss of the
host link must not leave gates high or notes indefinitely held. An auxiliary processor can
execute only its queued work during a host stall; continuing a complete performance after
a host failure is a separate feature.

### Firmware, portability and offline use

Provide CPU-only and simulated implementations of the same contracts. Their capabilities
may advertise weaker timing guarantees; do not claim hardware-timer precision for an
unmeasured desktop MIDI backend. Sequencer and Sound definitions do not change by device.

Packages can include per-target backend binaries and required firmware/kernel identities,
along with licenses and compatibility versions. A CPU build is not binary-compatible with
a DSP, nor does one vendor's DSP binary imply support for another. Board services control
firmware loading, peripheral assignment and restart, with an explicit recovery route.

Offline rendering uses the engine's CPU reference backend by default, with repeatability
for a fixed configuration. Live accelerated output must meet documented equivalence
tolerances. Hardware-only engines must declare whether offline rendering is supported;
report limitations instead of silently substituting another sound. A DAW host's audio
clock and callback constraints remain authoritative.

## Implementation sequence and validation

1. Define capability, clock and event contracts in the core; supply CPU-only and fake-clock
   services. Test timestamp conversion, drift, late input, equal-time ordering, cancellation,
   overflow, reconnects and safe releases deterministically.
2. Route desktop I/O through those contracts, preserving existing operation. Measure live
   latency and output scheduling; keep the control API outside this event path.
3. Add one board's timing backend after confirming wiring, peripheral ownership and
   firmware availability. Test MIDI/CV/audio alignment and worst-case jitter under CPU,
   storage, UI and bus load with physical loopback or measurement equipment. QEMU alone
   cannot verify this.
4. Implement backend discovery, conditional package resolution, reservations and one
   reference engine with CPU and simulated hardware backends before targeting an available
   real accelerator. Test selective installation, exact board/firmware matching, state round
   trips, explicit sound modes, reference audio tolerances, parameter timing, buffer sizes,
   resource contention and refusal when a CPU fallback or accelerator cannot meet the budget.
5. Exercise accelerator timeout/disconnect, firmware restart and CPU recovery with and
   without prepared capacity; measure complete latency and sustained deadline margin.
   Expand only after a backend shows a useful measured benefit.

## Open interface details

- Exact versioned C extension/ABI for plugin backend negotiation and service handles; keep
  format adapters separate and use standard plugin features where sufficient. Drafted in step 1 as
  `oscairn.hardware/draft`: accelerators listed by service ABI, reservations by a request
  (operation, bank, region, voices, parts, memory, transfer sizes, CPU fallback time), leases as
  host-issued numbers checked against the plugin that holds them, jobs on the plugin's own timeline
  (`steady_time`) and activation. It stays a draft until a real backend has used it.
- Clock estimation method, timestamp representation and endpoint-specific timing budgets.
  Narrowed in step 1: the timeline is an epoch and a 64-bit audio sample count, device times are
  64-bit ticks, and the mapping is a least-squares fit over the last 32 block-boundary readings with
  jumps restarting it. Endpoint budgets are open until step 2 measures desktop I/O.
- Per-board queue/cancellation protocol, minimum presentation delay and recovery behavior.
  Narrowed in step 1 to the host side's rules (bounded queue with a reserve for releases,
  cancellation from now plus the lead time, releases on a lost link, a new epoch after it); each
  board's queue depth, lead time and recovery are measured with its backend (step 3).
- Accelerator buffer ownership, sandbox transport and firmware/kernel packaging format. Buffer
  ownership answered for step 1: the host owns transfer buffers sized at reservation, copies a
  job's input in and its result out, and checks every size; one job per lease is outstanding.
  Plugins in worker processes see no accelerators until the worker protocol carries jobs. Firmware
  is named by identity in the capability record and in a package plan's requirements; its
  packaging format is open.

These need prototypes and measurements before an ABI is frozen. None requires moving
sequencer decisions into board firmware or making the UI responsible for timing.

## Hardware evidence

- [NXP i.MX 8M Mini specifications](https://docs.nxp.com/bundle/IMX8MMCEC/page/topics/imx_8m_mini_introduction.html):
  application Cortex-A53 cores and a Cortex-M4; separate execution domains.
- [NXP heterogeneous multicore framework](https://github.com/nxp-real-time-edge-sw/heterogeneous-multicore):
  Linux/RTOS communication, resource sharing and lifecycle infrastructure. This supplies
  transport mechanisms, not Oscairn's musical scheduling contract.
- [Ignite Linux documentation](https://github.com/MusicTribeOpenSource/0716-AAF/blob/main/Linux/README.md):
  i.MX8MM board support and an MCU firmware image omitted from the release. Verify the
  actual I/O owner before assigning timing work to the on-chip M4.

Sources initially checked 2026-10-28; FPGA/MODX references checked 2026-10-29. This RFC defines
the software design; it does not claim that any particular instrument has working Oscairn
acceleration or scheduled hardware I/O.
