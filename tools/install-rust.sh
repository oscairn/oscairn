#!/bin/sh
# Installs the Rust release samplepack pins (OSCAIRN_RUST_VERSION in CMakeLists.txt) with rustup,
# and rustup itself when it is missing, with what samplepack's rust-toolchain.toml lists, so the
# build itself downloads nothing. In GitHub Actions it puts cargo on the later steps' PATH.
#
#   tools/install-rust.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
version=$(sed -n 's/^set(OSCAIRN_RUST_VERSION \([0-9.]*\))$/\1/p' "$here/../CMakeLists.txt")
[ -n "$version" ] || { echo "install-rust: no OSCAIRN_RUST_VERSION in CMakeLists.txt" >&2; exit 1; }
if ! command -v rustup > /dev/null; then
	[ -x "${CARGO_HOME:-$HOME/.cargo}/bin/rustup" ] ||
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
	PATH=${CARGO_HOME:-$HOME/.cargo}/bin:$PATH
	[ -n "${GITHUB_PATH:-}" ] && echo "${CARGO_HOME:-$HOME/.cargo}/bin" >> "$GITHUB_PATH"
fi
rustup toolchain install "$version" --profile minimal --component clippy,rustfmt --target aarch64-unknown-linux-gnu
