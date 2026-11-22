# Vendored crates

## i-slint-backend-linuxkms 1.18.1

Slint's backend that draws straight to the display (DRM dumb buffers, else `/dev/fbN`) and reads
touch and keys through libinput, with no X server or Wayland compositor. Oscairn's Rust UI uses it
on devices (`ui/Cargo.toml`'s feature `kms`, through `[patch.crates-io]`).

- **Origin:** crates.io, `i-slint-backend-linuxkms` 1.18.1
  (`https://static.crates.io/crates/i-slint-backend-linuxkms/i-slint-backend-linuxkms-1.18.1.crate`,
  SHA-256 `0f519a8611a4982921e46e391015e5e40ceac5cc63813333ef91ada77df180d8`), from Slint's
  repository `https://github.com/slint-ui/slint` at commit
  `372cf0ee5577c3dfec309a45e7b778ba4e81b734`, `internal/backends/linuxkms`
  (`.cargo_vcs_info.json`). Every file is as published except the three the patch changes and
  the one it adds.
- **License:** `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`
  (its `Cargo.toml`; the texts in `LICENSES/`, kept as they are). Oscairn, AGPL-3.0-or-later,
  uses it, as all of Slint, under its GPLv3 option (the Setlist prototype's README, branch
  `experiment/rust-setlist`, on combining the two).
- **The patch:** `frame-cap.patch` (apply with `patch -p1` inside the crate's folder), in
  `fullscreenwindowadapter.rs`, and one line in `display/swdisplay.rs`:
  - `SLINT_KMS_MAX_FPS=N`: frames start at least 1/N s (less 2 ms, so a frame lands on every
    n-th vblank instead of slipping to the next) after the one before; a redraw asked sooner
    waits on a single-shot timer. Upstream renders on every request, at up to the display's
    rate (about 58 a second on a FANTOM-07). Measured there with the Setlist prototype
    scrolling: 92% of a core uncapped, 57% at 30. `oscairn-ui` sets it from the profile
    (`screens[].frameRate`, docs/rust-ui.md), so it is a setting, not only a variable.
  - `SLINT_KMS_FRAME_STATS` (any value): after each burst of frames (400 ms without one), one
    line on stderr: the frames, their rate, the gaps between them and the time each took to
    render and present. For measuring on a unit; nothing is kept when it is unset.
  - Not taken over from the experiment: a shadow buffer in cached memory (`SLINT_KMS_SHADOW`),
    which measured slower frames and brought the audio misses back on the unit.
  - `display/swdisplay.rs`: `is_write_combined_memory` allowed to be unused without the Skia
    renderer, its only caller, so the software-renderer build has no warning.
  - `display/swdisplay/dumbbuffer.rs` and the new `display/swdisplay/age.rs`: two dumb buffers,
    not three. Upstream's three come back three frames old, which the software renderer treats
    as a new buffer, so every frame was painted whole: measured on a FANTOM-07 at rest,
    13.5 ms of CPU a frame, 1.5 frames a second for the top bar's meters (2% of a core;
    41 frames and 553 ms in 28 s). With two, a buffer comes back two frames old
    (`SwappedBuffers`) and a frame repaints what it and the one before changed. The price:
    a frame waits for the flip before it to finish before it draws (upstream drew during it),
    at most one refresh and only when frames follow closer than a refresh after a present.
    `ui/src/tests.rs` renders by `age.rs` on any computer; the rest is compiled only for Linux.

To move to a newer Slint: vendor the new release's crate the same way, apply the patch (or
redo it by hand), update the version, hash and commit above and the `=` versions in
`ui/Cargo.toml`.
</content>
