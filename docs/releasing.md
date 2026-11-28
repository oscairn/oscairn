# Releasing

A release is, for now, one macOS disk image for Apple silicon,
`Oscairn-<version>-macos-arm64.dmg`, holding `Oscairn.app` (with its engine, plugin worker,
scan helper, bundled plugins, the Setlist and Settings apps, the themes and the
[preinstalled packages](#preinstalled-packages)), an `Applications` link to drag it to, and
`Licenses/`; and one tarball for each Linux CPU, `oscairn-<version>-linux-x86_64.tar.gz` and
`oscairn-<version>-linux-arm64.tar.gz` ([below](#the-linux-tarball)), each with the
[preinstalled packages](#preinstalled-packages), and the FANTOM-0's bundle
([below](#the-fantom-0-bundle)); the [release workflow](#run-the-release-workflow) builds them, with a
SHA-256 sums file for each.

## The version

The version is `project(oscairn VERSION …)` in the top-level `CMakeLists.txt`, with
`OSCAIRN_PRERELEASE` beside it while there is a pre-release label: `0.1.0` and `alpha.1` name
`0.1.0-alpha.1`. The app's `Info.plist` and what packages' `requires.oscairn` is checked
against take the numeric version only. Change both in a commit of their own before tagging.
CMake passes the whole release (`0.1.0-alpha.1`) to the code as `OSCAIRN_RELEASE`: it is what the
app, the Setlist and Settings apps (and their manifests' `appVersion`), the CLAP host
(`clap_host.version`) and the engine report, what `Oscairn --version` prints and what Settings ›
About shows. A release build also passes its commit (`OSCAIRN_SOURCE_COMMIT`), which About shows;
any other build says it is a development build.

## Cut a release

1. On `main`, with CI green, set the version and commit it.
2. Tag that commit and push the tag (the pipeline never creates a tag):

   ```sh
   git tag v0.1.0-alpha.1
   git push origin v0.1.0-alpha.1
   ```

   The tag must be `v` and the version, or the workflow stops.
3. Run [the Release workflow](../.github/workflows/release.yml) with `publish` on and that tag
   ([below](#run-the-release-workflow)). It builds the tag's own commit, and attaches the files and
   their sums to a **draft** release for the tag (marked a pre-release when the version has a label),
   with notes generated from the commits since the last release; nothing publishes it.
4. Download the files from the draft, try them on machines that have never built Oscairn (below),
   edit the notes, and publish the draft by hand.

## Run the release workflow

The workflow starts by hand only: Actions, Release, Run workflow, from the branch to take the workflow
file from (or `gh workflow run release.yml --ref <branch> -f targets=... -f publish=false`). Its inputs:

| Input | Meaning |
| --- | --- |
| `targets` | A comma list of `macos-arm64`, `linux-arm64`, `linux-x86_64`, `fantom-0`, or `all` (the default). `windows` is a later target and stops the run with an error saying so; so does any other name |
| `publish` | Off by default: the files are workflow artifacts only (kept 90 days). On: after every chosen target built, they are attached to the draft release of `tag`, which is made if it does not exist |
| `tag` | With `publish`: an existing tag, `v` and the version. The jobs then build that tag's commit instead of the branch's |

A target not in the list is not run; the others run in parallel, and one failing does not stop
the rest (but a publish waits for all, and does nothing if one failed). For one target, for
several, and for all:

```sh
gh workflow run release.yml -f targets=linux-arm64
gh workflow run release.yml -f targets=linux-arm64,linux-x86_64
gh workflow run release.yml -f targets=all
gh workflow run release.yml -f targets=all -f publish=true -f tag=v0.1.0-alpha.1
```

Each job ends with `SHA256SUMS-<target>.txt` beside its files, in the artifact `oscairn-<target>`
(`oscairn-macos-arm64-unsigned` for the dmg, which says so because it is signed ad hoc, not with a
Developer ID, and not notarized: the step that imports the signing secrets is not written yet,
[below](#signing-and-notarization)). A missing secret never fails a job.

| Target | Runner | Makes | Time |
| --- | --- | --- | --- |
| `macos-arm64` | `macos-15` | `Oscairn-<version>-macos-arm64.dmg` (`tools/release-macos.sh`) | 12.5 min without caches; 6.0 cold with them, 2.1 warm |
| `linux-arm64` | `ubuntu-24.04-arm` | `oscairn-<version>-linux-arm64.tar.gz` (`tools/release-linux.sh`) | 17.4 min without caches; 15.0 cold, 6.8 warm |
| `linux-x86_64` | `ubuntu-24.04` | `oscairn-<version>-linux-x86_64.tar.gz` | 16.7 min without caches; 14.0 cold, 5.5 warm |
| `fantom-0` | `ubuntu-24.04-arm`, or the larger runner the repository variable `OSCAIRN_FANTOM_RUNNER` names | `oscairn-<version>-fantom-0-bundle.tar.gz`, the USB-install bundle ([below](#the-fantom-0-bundle)) | cold: unknown, hours |

Not built yet: Windows, the OTA upgrade package of the FANTOM-0 (issue 219: its step in the job says
so and makes no file), signing and notarization.

## The build caches

CI (`ci.yml`) and the desktop jobs of the release workflow (`macos-arm64`, `linux-arm64`, `linux-x86_64`) keep
two things in the Actions cache between runs, through one composite action, `.github/actions/cache-build`:

- **ccache** for the C++ (`CMAKE_{C,CXX,OBJC,OBJCXX}_COMPILER_LAUNCHER=ccache`, set in the job's environment, so the
  release scripts need no change). ccache keys each object on the compiler's bytes, its flags and every header it
  reads, so a hit is the object the compiler would have made. The cache entry's own key names the job, the OS and
  CPU, the compiler's version and the build's variant (build type, sanitizer, the C++ CLAP host): the
  thread-sanitizer job's cache is its own and shares nothing with another job's.
- **cargo**, for the jobs that build `oscairn-ui` (the Linux releases; no CI job builds it): the registry and
  download folders (`~/.cargo/registry/{index,cache}`, `~/.cargo/git/db`) and the build folder's target folders
  (`ui-cargo`, `engine-core-cargo`, `samplepack-cargo`). Only `ui/` has crates from crates.io (531 in
  `ui/Cargo.lock`); `core/` has none, and what is vendored (`ui/vendor`, `core/clap-host/rust/vendor`) is a patched
  Slint backend and a few crates, a few hundred KB inside the repository, so only the target folders matter.
  The key names the job, the OS and CPU, the Rust release with its compiler's commit, and a hash of the lock files,
  `Cargo.toml` files and vendored folders: any change is a new entry, not an old one that keeps growing. Cargo's own
  fingerprints decide what is stale inside it; the project's own crates always rebuild, since a fresh checkout's
  files are newer. The release scripts keep these folders when `OSCAIRN_KEEP_CARGO=1` (the workflow sets it)
  instead of deleting `build-release/`.

Not cached, and why: the FetchContent downloads (JUCE, samplepack) are the configure step, 25 s with juceaide in
the Linux release's log, not worth an entry, and a kept JUCE checkout would skip its patch step
(`hosts/juce-patches`) when the patches change. The preinstalled packages (`tools/preinstalled.txt`): the fetch
reads the signed root and index and every piece on each run on purpose, and takes seconds (6 packages, 2 MB).
The Rust toolchain: `tools/install-rust.sh` takes under a minute. The CI image `oscairn-ci-rust:24.04` is not in
this repository and CI does not use it (the jobs run on the hosted runners).

**Running without caches.** CI: Actions, CI, Run workflow, "Use the build caches" off. Release: the same input,
`cache` (`gh workflow run release.yml -f targets=all -f cache=false`). Or set the repository variable
`OSCAIRN_CI_CACHE` to `off`: no run, pushes included, then uses them. Off, a job restores and saves nothing and sets
no compiler launcher. To start from nothing, delete the entries (`gh cache delete --all`).

**Who saves.** A run on `main` saves right after its build (not after its tests, so a slow or failed test run does
not lose it) and then deletes the older entry of the same job on `main`: one generation per job. A run on any other
branch or pull request saves the ccache when it restored nothing or compiled something new (ccache's miss count
of that run is not 0), and deletes its own older entry, so its next push starts from its own objects and not only
from `main`'s; an entry nobody restores is the first GitHub evicts. GitHub also deletes
entries unused for seven days. The workflows have `actions: write` for the deletion; a token without it (a pull
request from a fork) only skips that step.

**Sizes**, compressed, as the first runs on `pipeline-cache` saved them: ccache 17 MB (Linux build-and-test,
cpp-reference-host), 22 MB (macOS build-and-test), 83 MB (standalone-launch), 130 MB (thread-sanitizer), 26 to 30 MB
(the three releases); cargo 441 MB and 449 MB (the two Linux releases). The limits (`ccache-size` in the workflows)
are 300 MB a job, 500 MB for standalone-launch and the thread sanitizer: 2.8 GB if every one filled, against 0.35 GB
measured. With the cargo entries and the FANTOM-0 job's existing 1.2 GB entry, `main`'s share is 2.4 GB measured and
4.9 GB at the limits, inside GitHub's 10 GB a repository. Over the limit GitHub evicts the least recently used
first: the entries of branches and pull requests nobody restores, then `main`'s older keys; `main`'s current entries
are restored on every run and stay recent.

**Measured**, minutes per job, on branch `pipeline-cache` (issue 336); "without caches" is `main` on 2026-11-27
(runs 45922858746, 45942565773, 45947928752), the release row the first release run (45975663150):

| Job | Without caches | Cold run | Warm run |
| --- | --- | --- | --- |
| CI standalone-launch | 43.9 to 48.2 | 22.1 | 2.7 |
| CI cpp-reference-host | 14.8 to 16.7 | 19.9 | 10.2 |
| CI build-and-test, macOS | 24.7 to 27.3 | 30.4 | 23.2 |
| CI build-and-test, Linux | 19.6 to 20.0 | 25.1 | 17.1 |
| CI thread-sanitizer | 58.8 to 62.0 | 78.1 | 59.0 |
| Release macos-arm64 | 12.5 | 6.0 | 2.1 |
| Release linux-arm64 | 17.4 | 15.0 | 6.8 |
| Release linux-x86_64 | 16.7 | 14.0 | 5.5 |

The Build steps of the five CI jobs add up to 61 min without caches and 3 min warm. What is left is tests, which no
cache shortens: 57 min of the thread sanitizer's, 16 to 21 min of build-and-test. The Linux releases' remaining 5 to 7
min is mostly `oscairn-ui` itself (3.6 min, fat LTO, rebuilt every time). The cold runs' test steps also ran slower than
`main`'s that day, so their totals say little about the caches; a cold build is already shorter than none where one
object is compiled by several targets (JUCE's modules once per executable), which ccache serves from its own entries
within the run.

**Why the tests are the long pole.** On a warm run the Build step of build-and-test, the C++ host job and the sanitizer
is 3 to 4 min; the tests are 10 to 21 min (Linux 1007 s, macOS 1294 s, C++ host 589 s) and 80 min for the sanitizer,
and the longest single sanitizer test is 440 s. The earlier "after" row of the issue (build-and-test 29 and 28 min) was
the first run on `main` with the caches: a pull request's caches are its own and `main` could not read them, so that
run built cold (ccache 39% hits, Build 7 min); the next run on `main` restored it (78% hits, Build 3 min) and its
remaining time was the tests. The roughly 100 misses of every warm run are `core/control`'s sources, which each
commit touches.

**CI test shards.** `ci.yml` builds each tree once (`build`, `thread-sanitizer-build`, `cpp-reference-host-build`: the
only jobs that restore and save the ccache) and packs the build folder as an artifact (`.github/actions/tree-handoff`:
a zstd tarball without the object files, kept a day; the folder holds absolute paths of the runner's workspace, which are the same on every
runner of an OS). The test jobs unpack it into the same path and run one shard each: the thread sanitizer 10, Linux 4,
macOS 4, the C++ host 2. `tools/ci-ctest-shard.sh BUILD_DIR SHARDS SHARD EXCLUDE [COSTS [INCLUDE]]` deals the tests out
by cost, longest first to the shard with the least so far (`tools/ci-costs/*.txt`: seconds per test of 3 s or more, from
CI logs; a test not named counts 1 s; the costs decide only the balance, never which tests run), and runs the shard's
tests by exact name. Before it runs, it lists every shard's tests with `ctest -N` and fails the job if a test of the
full list is in no shard or in two, or if ctest selected more than a few fixture tests beyond a shard's own; a few
cheap fixture setup tests (`test-home-*`) run in every shard. Shard 0 of an OS also runs the steps that need the build
but no test (the clap-ext check, the production LV2 tests). To change a count, change the matrix and the number in the
script's arguments together; refresh a costs file from a CI log when the balance drifts (the slowest shard's minutes
show it). The limit on concurrent jobs: GitHub's documentation gives a Free organization 20 concurrent jobs, 5 of them
macOS (not verified for this organization; the API reports the plan "free"), so one run's 16 Linux test jobs (and up to 3 builds) and 4
macOS ones fit, and several runs at once queue.

**Measured**, job minutes (queue time is not in them). Before: `main` run 46113647163, the caches warm, the thread
sanitizer in four shards that each built. After: pull request run 46126666243, ccache 99% hits, the shards above.

| Job | Before | After |
| --- | --- | --- |
| build-and-test, Linux | 21.2 (setup 0.9, Build 3.3, tests 16.8) | build 1.9, then 4 shards of 4.8 to 7.1 |
| build-and-test, macOS | 26.0 (setup 1.0, Build 3.0, tests 21.6) | build 1.7, then 4 shards of 5.8 to 7.7 |
| cpp-reference-host | 13.9 (Build 3.2, tests 9.8) | build 1.1, then 2 shards of 6.1 and 6.3 |
| thread-sanitizer | 4 shards of 15.2 to 31.3 (each: Build 3.5 to 4.2, tests 11.2 to 26.5) | build 2.2, then 10 shards of 8.0 to 10.3 (each: setup 0.4, take the build 0.6 to 2.7, tests 6.2 to 8.9) |
| standalone-launch | 9.4 | 3.0 |

The longest path of a run without queueing is the changes job (0.1), the sanitizer build (2.2) and the slowest
sanitizer shard (10.3): about 13 min, from about 31. Wall times seen, queue included: 18.5 min on a run whose shards
started promptly, 26.5 min on one whose sanitizer shards waited up to 12 min for runners (other runs share the
organization's limit). The sanitizer's compute is about the same as before (93 against 99 runner minutes: the four
rebuilds are replaced by ten downloads of the 2 GB tree); only its wall time fell.

**Paths that skip the builds.** The `changes` job of `ci.yml` lists the files a change touches (a pull request against
its base, a push against the previous head) and `tools/ci-skippable.sh` says whether any of them can matter: Markdown
anywhere, `notes/`, `.trail/`, `docs/images/` and `ui/` cannot (`docs/`'s `.json` schemas are read by tests, so they
build). When none can, the build jobs, and so the test jobs, are skipped, and the last job, `CI result`, passes: it
is the one name a branch rule should require, since a skipped matrix job reports under no name a rule could list.
`ui/` is skipped because `ci.yml` builds `oscairn-ui` nowhere (`OSCAIRN_UI` is off; only the release workflow builds
it) and no C++ test reads it; the Rust CLAP host (`core/clap-host/rust`) is not under `ui/`, so its coverage is
unchanged. It also means a change to `ui/` is built and tested by no CI job at all today.

## The FANTOM-0 bundle

The `fantom-0` job runs `os/fantom-bundle.sh` ([the bundle](fantom-0.md#running-in-a-chroot-on-rolands-linux)):
Buildroot 2026.08.1 in the builder container (`os/docker/Dockerfile`, built on the runner each time),
in a folder (`OSCAIRN_OS_VOLUME` as a path, in place of the Docker volume `os/build.sh` uses by default).
The result is the root filesystem tarball, the one copied to the unit by USB memory and unpacked there.

What is realistic, and what is not known:

- The maintainer's cold build is "an hour or more" on a many-core Mac; a GitHub runner has four cores, and the
  build has never been run on one. A cold run may not fit the six hours a job may take. The job
  therefore stops the build at 5 h 35 min, saves the downloads and ccache to the Actions cache even then, and the next
  run starts from them: the first run may fail by design, and its re-run is the one to expect to finish.
  Only `dl/` and `ccache/` are kept (3 GB on the maintainer's machine); Buildroot's own toolchain (GCC)
  is built again each run. To skip the cold run, seed the cache from a build made elsewhere: a tarball of
  `dl/` and `ccache/` from the maintainer's builder volume, saved under a key that starts with
  `fantom-0-buildroot-`.
- Disk: the output folder was 15 GB on the maintainer's machine, with 1.6 GB of downloads. The job removes the
  runner's .NET, Android and Haskell toolchains first; if the runner still runs out, name a larger runner in
  `OSCAIRN_FANTOM_RUNNER` (repository Settings, Variables).
- The job uses an arm64 runner because the builder container and the bundle have only been built on arm64.
- Nothing in this job, or in its cache handling, has been run on GitHub yet. The first run is its test.

## A prebuilt base for the bundle

**State (issue 336, stage B).** Built and tried locally; nothing is published, run on GitHub, or enabled. The scripts:
`os/scripts/base-key.sh` (the key), `os/base-image.sh key|build|image|seed`, `OSCAIRN_BASE_MODE=base` in
`os/scripts/container-build.sh` (builds the 98 base targets `make show-targets` lists, refuses a volume that built
Oscairn, prunes `build/` to stamps), `os/docker/Dockerfile.base`, and `OSCAIRN_OS_BASE_IMAGE=<image> os/fantom-bundle.sh`,
which seeds an empty volume from the image first (the entry points `os/build.sh` and `os/fantom-bundle.sh` keep their
arguments). On GitHub, written and off: `.github/workflows/fantom-base.yml` (only `workflow_dispatch`; it pushes only
with its `publish` input on and from `main`) and the Release workflow's `fantom_base` input (empty by default).
Tried on this Mac with a copy of a full bundle build's volume: pruning `build/` from 8.4 GB to 8 MB (with `host/` 741 MB
and `target/` 172 MB kept) lets Buildroot finish the bundle, building only `oscairn-juce`, `oscairn-samplepack`,
`oscairn-ui-crates` and `oscairn` and then the root filesystem, never a base package; the tarball is 74.9 MB, and the same size from the seeded volume below;
the image made from that base is 839 MB of content; a bundle build from an empty volume
seeded from it, with a cold ccache and no downloads, took 11.4 min (seed copy included). Not tried: building the base
from nothing (hours), pushing and pulling through the registry, the workflow on a runner. The owner decides whether to
publish the package (`ghcr.io/oscairn/oscairn-os-base`) and sets its visibility and the repository's access.
The `fantom-0` job builds the whole Buildroot tree each run: the first release run
(a cold one, nothing in the Actions cache yet) took 67.9 min; every run compiles the cross toolchain again, since only `dl/` and ccache are kept, and then every base package.
The plan is to build everything that is not Oscairn once per Buildroot configuration, publish it as a container image, and let
the job build only Oscairn's four packages (`oscairn`, `oscairn-juce`, `oscairn-samplepack`, `oscairn-ui-crates`) and the
root filesystem on top. The same split is the upgrade build of issue 219: the Oscairn layer alone while the base is unchanged.

What was checked: in the builder container, Buildroot 2026.08.1 with `oscairn_fantom_bundle_defconfig`, `make show-targets`
lists every package the configuration builds (the toolchain, glibc, gcc-final, the host tools, Xorg, libinput, alsa-lib and the
rest, `rootfs-tar`, and Oscairn's four), and `make sdk` exists. Nothing below it has been run.

**The base.** One image, `ghcr.io/oscairn/oscairn-os-base:<key>`, `FROM oscairn-os-builder` (so Buildroot's host requirements
and the Rust release are in it) with `/work` holding the Buildroot clone and the output folder after building every target that
`show-targets` lists except `oscairn*` and `rootfs-*`, from the same defconfig the bundle uses. The same `.config` for the base and the
bundle is the point: Buildroot never rebuilds a package because an option changed, so a base built from a different
configuration (the dependencies listed by hand, say) could differ from the bundle's silently. Building with
the full configuration and leaving out only Oscairn's packages has no such gap, and a dependency the list missed would simply be
built later in the job. The output folder is pruned before the image is made: `build/` keeps only each package's
`.stamp_*` and `.files-list*` files, so that Buildroot finds the package built and does not need its sources or objects
(`host/`, `staging/` and `target/` are kept whole). Unverified, and the first thing to test: that Buildroot builds
the four packages and the root filesystem from a `build/` reduced like that. If it does not, the fallback is to keep `build/` whole
(15 GB on the maintainer's machine, so in several layers: a GHCR layer may not pass 10 GB).

**The key.** `fantom-base-<sha256 of the files below, 16 hex digits>-<arm64>`, computed by a script the job and the
workflow share: `os/buildroot/configs/oscairn_fantom_bundle_defconfig`, `os/buildroot/external.desc` and `external.mk`,
`os/buildroot/Config.in` and each package's `Config.in` (they select the dependencies, so they change the configuration),
`os/buildroot/board/fantom-bundle/patches/**` and `board/common/linux.fragment`, `os/docker/Dockerfile` (the Rust release
and the host packages), and `os/scripts/container-build.sh` (the Buildroot version). Left out, because they change nothing the base
holds: the overlays, `post-build.sh` and the four Oscairn packages' `.mk`, hash and patch files, which run when the root
filesystem is made. A package version or an option changed anywhere above is a new key and a new base, never a stale one.

**Making it.** A new workflow, `fantom-base.yml`, on `ubuntu-24.04-arm`: it runs on a push to `main` that touches the files in the
key, on `workflow_call` (the release workflow's `plan` job calls it when `fantom-0` is chosen, before the bundle job), and by hand.
It computes the key, stops if the image exists (`docker buildx imagetools inspect`), and otherwise builds as the
`fantom-0` job does today (the free-space step, `dl/` and ccache in the Actions cache, the build stopped at 5 h 35 min and
continued by the next run), prunes, builds the image, and pushes it with `GITHUB_TOKEN` (`packages: write`). It publishes only from
`main`; a branch builds the image without pushing it. This is the first thing under the organization's packages: the first run
on `main` publishes it, and the package's visibility (public) and the repository's access to it are set once in the
organization's package settings.

**Using it.** The `fantom-0` job pulls the image for the current key, copies `/work` into its folder
(`docker run --rm -v "$work":/dst <image> cp -a /work/. /dst/`), and runs `os/fantom-bundle.sh` as now: `container-build.sh`
applies the defconfig, finds the base built and builds the four packages and the root filesystem. Its own ccache
(Oscairn's compile, JUCE) stays in the Actions cache. A missing image fails the job with the workflow to run, rather than
building the base inside the release. The time is then Oscairn's own build, which has not been measured on this runner (an estimate, from the
Linux arm64 release's 15 min cold: tens of minutes, not the full Buildroot build).

**The 10 GB.** The base lives in the container registry, not in the Actions cache, so it does not count toward the
repository's 10 GB and nothing evicts it; the Actions cache then holds only the base workflow's `dl/` and ccache (1.2 GB now,
read only when a base is rebuilt) and the job's small ccache. Old bases are removed by hand or by a retention rule on the package.

**Why not the SDK.** `make sdk` gives a relocatable toolchain and sysroot, and Oscairn could then be built with cmake
outside Buildroot and added to a base root filesystem. That is a second way to build the Oscairn package, to be kept equal to
`oscairn.mk` (its cmake options, the vendored Rust crates, the patched JUCE, the install hooks); the plan above keeps the one.

## The first run

Try the dmg on a scratch home and data folder, never your own: with the app copied out of the
dmg, run `Oscairn.app/Contents/MacOS/Oscairn` with `HOME`, `OSCAIRN_DATA_HOME` and
`OSCAIRN_RUNTIME_DIR` set to folders under one scratch folder. The Library says it is setting up
the instruments that come with Oscairn; `packages.preinstalled` on the engine's control socket
then ends `done`, each package `installed`.

## Try it on a clean Mac

Scripted first-run checks of the downloaded app (launching it, snapshots, driving it over its
socket) start it with `--audio null`, for example
`/Applications/Oscairn.app/Contents/MacOS/Oscairn --audio null --snapshot home.png`. Its engine
then opens the null device, which writes nowhere, and no MIDI device, and the app opens none of its
own (no control surface or controller feedback port): no virtual output device has
to be set up by hand, and nothing a script does can sound. Listening to it is a check of its own,
without the option.

## Build one locally

```sh
tools/release-macos.sh
```

It needs a clean tree (a release is built from a commit, so its source is that commit),
CMake, Ninja and Xcode's command-line tools, and the network the first time (JUCE). It deletes
and rebuilds `build-release/`, builds the app in Release for arm64 and macOS 13.3 or later
(the oldest with floating-point `std::to_chars`), checks that no program or plugin in it links
anything but the system's libraries or has an rpath, signs it, and writes
`build-release/Oscairn-<version>-macos-arm64.dmg`.

codesign accepts only code in `Contents/MacOS`, where the app looks for its `themes/` and
`Apps/`; the script moves them to `Contents/Resources` and leaves links in their place.

## The Linux tarball

```sh
tools/release-linux.sh
```

On a Linux machine (x86_64 or arm64; the tarball is for the CPU it is built on), with a clean
tree, CMake, Ninja, a C++20 compiler, Python 3, the network the first time (JUCE, and the
preinstalled packages) and the development packages JUCE needs: `libasound2-dev`,
`libcurl4-openssl-dev`, `libfreetype-dev`, `libfontconfig1-dev`, `libx11-dev`, `libxext-dev`,
`libxinerama-dev`, `libxrandr-dev`, `libxcursor-dev` and `libxi-dev`. `OSCAIRN_BUILD_JOBS` limits the build's
parallel jobs. It deletes and rebuilds `build-release/` and writes
`build-release/oscairn-<version>-linux-<arch>.tar.gz`, where `<arch>` is `x86_64` or `arm64`
(the package platforms `linux-x86_64` and `linux-arm64`).

The tarball unpacks to one folder, `oscairn-<version>-linux-<arch>/`, that runs from wherever it
is put:

```
bin/Oscairn, oscairn-engine, oscairn-plugin-worker, oscairn-plugin-scan, oscairn-ui
bin/plugins/       the bundled plugins (.clap)
bin/themes/        the built-in themes
bin/Apps/          the Setlist and Settings apps
share/applications/oscairn.desktop, share/icons/hicolor/1024x1024/apps/oscairn.png
Licenses/          as in the dmg
LICENSE, README.txt  (README.txt names the packages it needs to run)
```

`oscairn-ui` is the Rust UI, which the app starts as its interface ([Rust UI](rust-ui.md)); the script builds it
(`-DOSCAIRN_UI=ON`) and fails when it is not beside the app. The Rust CLAP host is linked into
`oscairn-engine` and `oscairn-plugin-worker`, so it is not a file of its own.

This is the layout the build leaves beside the program, and the one the app looks in first
(`hosts/standalone/Main.cpp`, `Theme.cpp`, `AudioEngine.cpp`): it finds all of it from its own
file (`Executable.h`), however it was started. The binaries are stripped, and the script fails
when one needs a library the machine lacks or has a path into the build tree. Unlike the dmg,
the program links the system's libraries (ALSA, libcurl, FreeType, fontconfig, X11), which
`README.txt` lists as packages; the build must be on the oldest system the release is to run
on, which the GitHub runners' Ubuntu 24.04 sets.

Each tarball has the preinstalled packages ([below](#preinstalled-packages)) in `bin/Preinstalled`,
where the engine looks beside the programs (`hosts/audio/PreinstalledFolder.h`), and their licenses in
`Licenses/Packages/`; the script fetches them for `linux-<arch>`, so every package in
`tools/preinstalled.txt` needs a build for both Linux CPUs, or the release fails ("has no build
for linux-x86_64").

Not made: an AppImage, a .deb or a Flatpak, a 32-bit or Wayland-native build. The workflow's
job writes the sums ([above](#run-the-release-workflow)); the script does not.

Try one as the macOS first run above: unpacked in a scratch folder, with `HOME`,
`OSCAIRN_DATA_HOME` and `OSCAIRN_RUNTIME_DIR` set to folders under it, and
`bin/Oscairn --audio null` (a desktop, or `xvfb-run` with `--snapshot home.png`).

## Preinstalled packages

A release ships Oscairn's own published packages, which the engine installs at its first start
on each data folder ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#preinstalled-packages)).
[`tools/preinstalled.txt`](../tools/preinstalled.txt) lists them, each a package ID and version
at https://packages.oscairn.org; change it in a commit of its own, to the newest stable versions,
before tagging. The release script runs [`tools/preinstall-fetch.py`](../tools/preinstall-fetch.py),
which downloads the official repository's signed root and index and the listed packages' pieces
for the platform ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#pieces-the-format))
into `Oscairn.app/Contents/Resources/Preinstalled` as a repository snapshot, and fails the release
when anything does not verify: the root against the official root key, the index and each piece
by hash, every signature in the archive the pieces make for the platform by the publisher key
the index names, a version yanked or without a `macos-arm64` build, or an asset library a listed
package needs and the list lacks. It needs the network, and the pieces are never committed. To
check a list without building:

```sh
python3 -I tools/preinstall-fetch.py --oscairn-sign build/core/control/oscairn-sign \
    --platform macos-arm64 --out /tmp/preinstalled --licenses /tmp/preinstalled-licenses
```

The Linux tarballs run it with `--platform linux-<arch>` into `bin/Preinstalled` ([above](#the-linux-tarball)).

The 2026-11-11 set adds 11.8 MB of archives to the app (Analog's factory patches 7.4 MB of it).

## Signing and notarization

Without settings the app is signed ad hoc (`codesign -s -`), which Apple silicon needs to run
it at all. Gatekeeper refuses an ad hoc signed app downloaded from the web. On macOS 15 and
later, Control-click and Open no longer gets past it: the release notes must say to open the
app once, then choose Open Anyway in System Settings, Privacy & Security, or to run
`xattr -dr com.apple.quarantine /Applications/Oscairn.app`.

Once there is an Apple Developer account:

| Variable | Effect |
| --- | --- |
| `OSCAIRN_SIGN_IDENTITY` | A Developer ID Application identity in the keychain (`security find-identity -v -p codesigning`). Signs every program, plugin and app with it, with the hardened runtime and `tools/release-macos.entitlements` (audio input, and loading plugins signed by others) |
| `OSCAIRN_NOTARY_PROFILE` | A profile saved with `xcrun notarytool store-credentials`. With an identity, the dmg is signed, notarized and stapled |

In the workflow these need the certificate imported into a temporary keychain and the
notary credentials stored from repository secrets first; that step is not written yet (the
owner holds the secrets), so the workflow's dmg is always the ad hoc signed one, named `-unsigned`.
Neither path has been tried.

## Licenses

`Licenses/` (in the dmg, and in the app's `Contents/Resources/Licenses`) holds Oscairn's
AGPL-3.0, `CREDITS.md`, `SOURCE.txt` (where the source of this exact commit is), and the
notices of what is built in: CLAP (MIT), Oscairn's CLAP extensions (MIT), miniz (MIT),
Monocypher (BSD-2-Clause or CC0), Lucide (ISC), Inter (OFL 1.1, also in each theme's
`fonts/`), JUCE's license and its `JUCE.spdx.json`, and in `Licenses/JUCE/` the libraries
JUCE's modules compile into the app: zlib, HarfBuzz, SheenBidi, LunaSVG, PlutoVG, libpng,
libjpeg (the Independent JPEG Group's credit), libwebp, FLAC, libogg, libvorbis, Opus,
opusfile, libopusenc and the VST3 SDK. sheredom's json.h is public domain and needs none.
The preinstalled packages' own licenses go in `Licenses/Packages/<package ID>/`: each package's
license texts and notices as it carries them, and `PACKAGE.txt` with its license expression and
its source's address and revision, which `SOURCE.txt` points to; under the GPL that is the
directions to the source the license asks for, so a package's source repository must stay
available as long as the release is offered. The LV2 host (jalv's ISC) is not in the macOS
app or the Linux tarball. The tarball's `Licenses/` is made by the same
[`tools/release-licenses.sh`](../tools/release-licenses.sh). When the app starts using another JUCE module, check `JUCE.spdx.json` for what it vendors
and add those notices to the script.

`SOURCE.txt` points at the commit on GitHub, so a dmg is only complete once its commit is
pushed: build releases from a pushed tag.
