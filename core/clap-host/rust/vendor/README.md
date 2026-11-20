# Vendored crates

The engine's Rust (core/Cargo.toml's workspace) builds offline everywhere, the device's image included, so its two
dependencies are here rather than fetched: one is not on crates.io, and the other is one small crate with no
dependencies of its own.

## clap-sys 0.5.0

Rust bindings for the CLAP C API: the CLAP types the Rust host (`core/clap-host/rust`) uses, and the ones
oscairn-clap-sys's bindings are written against.

- **Origin:** crates.io, `clap-sys` 0.5.0
  (`https://static.crates.io/crates/clap-sys/clap-sys-0.5.0.crate`, SHA-256
  `a76abbdb2907f6fd97fb6bc0b7be96b77d328f2dd9669d1075cc03369ed22154`), from
  `https://github.com/micahrj/clap-sys` at commit `25d7f53fdb6363ad63fbd80049cb7a42a97ac156`
  (`.cargo_vcs_info.json`). Every file as published but the `.github` folder (its CI), left out.
- **License:** MIT or Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).
- **Note:** it is CLAP 1.2.2's; Oscairn's headers (`core/clap-host/third_party/clap`) are 1.2.10. The extension IDs
  and structures the host uses are the same in both; the version the host announces is the headers'
  (`host.rs`'s `CLAP_VERSION`, checked against `version.h` by the test `clap_version_is_the_headers`).

## oscairn-clap-sys

Raw bindings for Oscairn's CLAP extensions (`core/clap-ext/include/oscairn-clap`), from the oscairn-clap repository
(`https://github.com/oscairn/oscairn-clap`, MIT, `LICENSE`), its `rust/oscairn-clap-sys` at commit
`6f27cdd2fd36176952e2211e825ea751e8195b2c`.

- `src/lib.rs` and `src/bindings.rs` as they are there; `Cargo.toml` is its manifest without the bindgen feature (and
  its build script, which only regenerates), with clap-sys taken from beside it instead of crates.io.
- The bindings were generated from the same headers as `core/clap-ext/include/oscairn-clap` holds: the host crate's
  test `extension_headers_unchanged` fails when those change. Then regenerate them in oscairn-clap (its `generate`
  feature), copy the two files here and update the test's hash.
