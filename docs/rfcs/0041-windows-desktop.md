# RFC 0041: Windows desktop, process and IPC support

Status: proposed, 2026-11-11. Builds on the Control API and its local transport
([RFC 0003](0003-control-api.md)), plugin worker isolation ([RFC 0010](0010-plugin-worker-isolation.md),
in progress on macOS), reliability levels ([RFC 0027](0027-reliability-levels.md)), resource budgets
([RFC 0025](0025-resource-budgets.md)), the system base's real-time tuning
([RFC 0035](0035-system-base.md)) and packages and their platforms
([RFC 0006](0006-packages-and-portability.md), [RFC 0031](0031-package-repository-and-distribution.md)).

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The core libraries built with MSVC, and the core and control tests passing in one
  process: the engine in the app, plugins in-process, no workers
- [ ] 2. The desktop app on Windows: play and edit a Performance with bundled plugins
- [ ] 3. The cross-platform process, shared-memory, wake and local-IPC layer; the engine and
  plugin workers as separate processes again, the control socket over a named pipe with
  per-user access
- [ ] 4. Real-time audio on Windows: MMCSS thread priority, timer resolution, page locking
- [ ] 5. Plugin windows embedded by HWND, and the scanner's isolated helper processes
- [ ] 6. `windows-x86_64` builds in packages and plugin CI; the Windows CI job green, including
  the Dexed, Surge XT and sfizz forks building CLAP for Windows

## Problem

Oscairn's desktop app runs on macOS and Linux. Its UI and audio already use JUCE, which supports
Windows (WASAPI, ASIO), and some Windows paths exist: plugin loading uses `LoadLibraryW`
(`core/clap-host/src/Internal.h:48`), the CLAP gui code selects `CLAP_WINDOW_API_WIN32`
(`core/clap-host/src/Host.cpp:418`), and discovery knows `%COMMONPROGRAMFILES%\CLAP` and `;`
path lists (`core/clap-host/src/Discovery.cpp:51,69`). But the app does not run there, because
everything around the plugins is POSIX:

- The desktop app runs the engine as a separate managed process with `posix_spawn`, a file lock
  and a Unix socket (`hosts/process/EngineProcess.cpp:88-122`), and each plugin in a worker
  process started the same way, fed a socket on descriptor 3 and shared memory passed over it
  (`core/plugin-host/src/WorkerHost.cpp:203-208,394`, `core/plugin-host/src/worker_main.cpp`,
  RFC 0010).
- The control channel is a Unix domain socket that rides descriptors on messages with
  `SCM_RIGHTS` (`core/control/src/local_socket.cpp:83-93,172-176`), at mode 0600 under a
  user-owned 0700 directory (`core/control/include/oscairn/local_socket.h:33`).
- The worker block's wait wakes on a futex (Linux) or `os_sync_wait_on_address` (macOS)
  (`core/plugin-host/src/Worker.cpp:29-58`); real-time priority is a macOS time constraint or
  Linux `SCHED_FIFO` (`core/plugin-host/include/oscairn/realtime.h:25`). There is no Windows
  branch in either.
- Isolated plugin scanning runs a helper process and is POSIX-only: on Windows it reports
  "Isolated scanning currently supports Linux and macOS" (`core/clap-host/src/Discovery.cpp:189,281`,
  with the same `setrlimit`-guarded scanners in `core/clap-host/tools/scan.cpp:24` and
  `core/plugin-host/src/scan.cpp:24`).
- Host windows for embedded plugin editors are built for macOS only; `Window.h:10` says "macOS
  (Cocoa) so far" and `Window.cpp:2` notes Linux embedding is not built either.
- The data folder is `~/Library/Application Support/Oscairn` or the XDG one
  (`core/control/src/library.cpp:94-104`, `core/plugin-host/src/assets.cpp:84-94`); Windows has
  neither `HOME` set by default nor XDG.

Keeping these as `#ifdef`s spread through the code has already started: `Discovery.cpp`,
`Host.cpp` and the test fixtures each carry their own guards, and the CI matrix is
`ubuntu-24.04` and `macos-15` only (`.github/workflows/ci.yml:14-16`).

## Outcome

The desktop app runs on Windows 10 and 11, x86_64: the same Performances, Sounds, plugins and
packages, with worker isolation and crash recovery like macOS and Linux, a Windows CI job, and
`windows-x86_64` plugin builds in packages. First as one process (the engine in the app, plugins
in-process), then with the full process model, as [RFC 0040](0040-ios-developer-builds.md) does
for iOS.

## Design

### One process first

Steps 1 and 2 run everything in the app's process, which the code already has as a mode: the
desktop app's engine connection is in-process when given no helper and no socket
(`hosts/standalone/Engine.h:25`), and the engine keeps `--in-process` as an alternative to
spawning ([control protocol](../control-protocol.md#local-transport-and-client)). Plugin workers
are switched off the way RFC 0040 describes for iOS (`OSCAIRN_PLUGIN_WORKER=`), so a plugin's
crash takes the app down and the reliability level tops out below `instrument` (RFC 0027),
shown in the status. Scanning has no Windows path today: on Windows `scan()` lists nothing and
reports "Isolated scanning currently supports Linux and macOS" (`core/clap-host/src/Discovery.cpp:281-283`).
Step 1 adds an in-process scan there, calling `inspect()` on each binary as the helper does, with
no crash or hang isolation until step 5.

What step 1 actually fixes is everywhere the code assumes POSIX headers and calls: sockets,
`mlock` (`core/engine/src/recorder.cpp:37,450`), `dlfcn`, signals, `getpid`. The MSVC build
makes that a list instead of a guess.

### One small cross-platform layer

Four primitives, one interface, no `#ifdef`s outside it. Today they live inside
`EngineProcess.cpp`, `WorkerHost.cpp`, `Worker.cpp` and `local_socket.cpp`; the plan is one
small internal library (`core/os/`, an `oscairn_os` target) that all four call:

| Primitive | POSIX today | Windows |
| --- | --- | --- |
| Process: spawn, terminate, wait; children die with the parent; a memory limit | `posix_spawn` with the child in its own process group (`hosts/process/EngineProcess.cpp:106`), a stdin pipe the helper watches and ends itself on (`core/clap-host/src/Discovery.cpp:178-188`), `setrlimit(RLIMIT_AS)` (`core/plugin-host/src/worker_main.cpp:29`) | `CreateProcessW`; a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` closes the group-and-pipe trick and gives the limit as the job's per-process memory limit, which covers commit rather than address space — closer to what the supervisor enforces anyway |
| Shared memory | `shm_open` + `mmap` + `shm_unlink` (`core/plugin-host/src/WorkerHost.cpp:394`) | a page-file `CreateFileMapping` + `MapViewOfFile`; a handle, not an fd |
| Wait and wake on one shared word | futex / `os_sync_wait_on_address` (`core/plugin-host/src/Worker.cpp:29-58`) | `WaitOnAddress` / `WakeByAddressSingle` / `WakeByAddressAll` on the same 32-bit word in shared memory: the same primitive Windows documents for exactly this |
| Local IPC with handle passing | `AF_UNIX` `SOCK_STREAM` + `SCM_RIGHTS` (`core/control/src/local_socket.cpp`) | a named pipe under the user's pipe directory, handles passed with `DuplicateHandle` into the peer via the broker's own process, or `AF_UNIX` (which Windows supports but with no descriptor passing) if the passing can move beside the socket rather than inside it |

The framing does not change: one compact JSON document plus LF, the byte limits and the client
cap stay as they are ([control protocol](../control-protocol.md#local-transport-and-client)).
Only the transport's ends and the passing change. Named pipes (`\\.\pipe\...`) share one
namespace for every user on the machine, so the name alone protects nothing: the pipe's name
carries the user's SID to avoid clashes, and its security descriptor (below) does the protecting.

Which handle-passing mechanism — named pipes with `DuplicateHandle`, or `AF_UNIX` plus a second
channel — is the layer's first decision, made with a working probe of both against the existing
`apps.connect` flow (the descriptor-riding connection,
[control protocol](../control-protocol.md#local-transport-and-client)). The rest of the layer is
settled.

### The control endpoint's security

The Unix endpoint is 0600 in a directory only the user can enter, and the server refuses an
existing path (`core/control/include/oscairn/local_socket.h:33-34`,
`core/control/src/local_socket.cpp:404`). The named pipe matches it with a security descriptor
that grants read and write to the current user and to `SYSTEM` alone, denies everything else,
and the same refuse-an-existing-endpoint rule. The same-user trust boundary of the Unix socket
is kept exactly: this is a control interface, not a sandbox, and it says so in the protocol
documentation, which gains a Windows paragraph.

### Real-time audio and timers

`makeRealtime` (`core/plugin-host/src/Worker.cpp:60-71`) gains its third branch:
`AvSetMmThreadCharacteristics` with the `Pro Audio` task, and `AvSetMmThreadPriority` above the
pool threads. The engine's and worker's block pacing asks Windows to hold the system timer at
1 ms while playing (`timeBeginPeriod`; how that behaves on current Windows 11, where the
kernel's timer granularity changed, is to check at step 4). Page locking (`mlockall`,
`core/plugin-host/src/plugin_host.cpp:254-262`, and the recorder's `mlock`) becomes best-effort
`VirtualLock`, reported as "memory not locked" exactly as today when it fails. `oom_score_adj`
has no Windows equivalent and is reported as not done, which the code's existing
return-what-could-not-be-done shape already handles (`applyMemoryPolicy`,
`core/plugin-host/include/oscairn/plugin_host.h:171`).

### Plugin loading, scanning and windows

- Loading: `LoadLibraryW` + `clap_entry` from `GetProcAddress` is already built
  (`core/clap-host/src/Internal.h:48-50`); step 1 runs it for real.
- Scanning: `runHelper` becomes the job-object child of the new layer — timeout, stderr tail and
  the never-outlive-the-host rule — and the `#ifdef` at
  `core/clap-host/src/Discovery.cpp:281` goes away. The scanner tools' `RLIMIT_AS` becomes the
  job's memory limit (`core/plugin-host/src/scan.cpp:23-24`).
- Windows: `CLAP_WINDOW_API_WIN32` and floating windows already work; what is missing is the
  host's own window with an `HWND` for `gui->set_parent`, the Windows peer of `Window.mm`. JUCE
  already owns such windows in the desktop app, so this is a `Window` implementation whose
  `view()` is the content `HWND`, handed to the plugin through `gui->set_parent` as CLAP's `gui.h` describes for `win32`, which
  embeds its window with [`SetParent`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setparent).
  Resize, scale and the DPI difference between logical and physical pixels follow CLAP's
  `win32` (logical size) API.

### Paths, names, line endings and the `windows-x86_64` platform

- Data folder: `%APPDATA%\Oscairn` through the Known Folder API
  (`SHGetKnownFolderPath(FOLDERID_RoamingAppData)`), with `OSCAIRN_DATA_HOME` still winning
  (`core/control/src/library.cpp:95`). `%LOCALAPPDATA%` for the plugin store would split one
  library across two roots; it stays one folder.
- Names: `Library::newFolder` already maps everything outside letters, digits, space, `-` and
  `_` to `_` (`core/control/src/library.cpp:371-372`), so no Sound or Performance folder made by
  Oscairn holds a Windows-forbidden character. Three gaps remain and are fixed at the same place:
  Windows reserved device names (`CON`, `PRN`, `NUL`, `COM1`… case-insensitive) collide with the
  sanitizer's output; Windows strips trailing dots and spaces from a name the sanitizer lets
  through; and a name that differs from another item's only in case is one folder there and two
  here, so the taken-name check (`:380`) compares case-insensitively always, not only on Windows.
  Installed libraries and archives can carry names from a case-sensitive machine: installation
  refuses two items whose folders differ only in case.
- Line endings: the protocol already accepts CR before LF as whitespace
  ([control protocol](../control-protocol.md#local-transport-and-client)). Files must never be
  opened in text mode on Windows, which would turn LF into CRLF; whether every writer opens its
  files binary today is to check at step 1, and any that does not is fixed then.
- Platforms: `windows-x86_64` is already in the platform list the store and the signing tool
  accept (`../plugin-porting.md`, "Packages and signing"; `core/control/src/package.cpp:87` reports
  `windows`). Nothing in the format changes; step 6 builds and signs such builds
  (`oscairn-sign … windows-x86_64=…`, [RFC 0031](0031-package-repository-and-distribution.md))
  and exports carry them as any other platform's build.

### CI and the plugin forks

- `.github/workflows/ci.yml` gains a `windows-latest` job: MSVC through the developer command
  prompt, the same configure-build-ctest shape, `-DOSCAIRN_LV2=OFF` (LV2 production testing
  stays on Linux and macOS as today). The steps' POSIX-only fixtures (a hung helper, a killed
  worker) are excluded until step 3 lands them, then unexcluded; the job's point is that the
  guard list shrinks every step, not that it hides.
- The forks build for Windows in their own repositories' CI: [RFC 0014](0014-fm-engine.md)'s Dexed
  (`oscairn/dexed`), [RFC 0015](0015-analog-modeling-engine.md)'s Surge XT (`oscairn/surge`) and
  [RFC 0013](0013-awm2-class-sample-engine.md)'s Oscairn Sampler (`oscairn/sfizz`). All three
  are plugin CLAP builds whose upstreams ship Windows binaries, so the work is the forks' CLAP
  target, pinned and built per platform as the CI template
  [RFC 0035](0035-system-base.md) plans (not built yet) would do; which of the forks' Windows targets already build
  is to check at step 6.

## Build order and gates

| Step | Gate |
| --- | --- |
| 1 | A clean Windows runner builds the core libraries with MSVC and `ctest` passes in-process: the engine, the in-process host, the control and library tests, no workers, no `dlfcn`, no `shm_open`. |
| 2 | On a Windows 11 machine: the desktop app starts, a Performance with bundled plugins plays over WASAPI and edits, and its Sounds save under `%APPDATA%\Oscairn`; the status shows the reliability level below `instrument`. |
| 3 | The engine and one worker per plugin run as job-object children; the `local_socket` suite passes over the new transport, `apps.connect` hands its peer a real handle, and a second user's process on the same machine is refused at the pipe. A killed engine's workers die; the crash and hang fixture tests pass again. |
| 4 | The deadline-miss counter stays at zero for 10 minutes of a multi-part Performance at 48 kHz, 128 frames; the status reports the audio thread in the `Pro Audio` class, and says plainly what could not be locked. |
| 5 | A plugin editor opens embedded in the app's own window, resizes with it, and survives a DPI change; a hung scanner helper does not outlive the scan. |
| 6 | The Windows CI job is green in a pull request; a signed `windows-x86_64` build of at least one fork plugin installs from a plugin package and plays in the Windows app. |

## Open questions

- Named pipes with `DuplicateHandle`, or `AF_UNIX` beside a handle-passing channel: the probe
  in the design's first section decides.
- Whether to enable JUCE's ASIO support in the desktop app (the ASIO SDK has its own license
  terms, to check against the AGPL), and what the Settings app's Audio page shows
  when a device offers both WASAPI and ASIO.
- Whether the job-object commit limit or a per-allocation cap matches RFC 0025's resident-memory
  budget better; measure against `worker-bench` at step 3.
- 32-bit and ARM64 Windows: only x86_64 is planned; whether ARM64 matters enough to keep the
  layer free of x86 assumptions costs one extra CI job, not a design change.

## Ruled out

- MinGW or Clang as the primary Windows compiler: the CI job is MSVC, the forks' upstreams ship
  MSVC builds; MinGW is an unsupported build if someone wants it.
- Cygwin or WSL for the desktop app: the point is native windows, MMCSS and HWND embedding.
- Windows ARM64 and 32-bit in this RFC's scope: [Open questions](#open-questions).
- LV2 hosting on Windows in this RFC's scope: the production LV2 suite is tested on Linux and
  macOS only, and stays out of the Windows CI job.
