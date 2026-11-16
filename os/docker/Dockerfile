# Build environment for the Oscairn OS images: Buildroot's host requirements, and QEMU to
# boot the test image. Debian 13 (trixie).
FROM debian:trixie

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bash bc binutils build-essential bzip2 ca-certificates cpio file g++ gcc git gzip \
        libncurses-dev make patch perl python3 rsync sed tar unzip wget xz-utils \
        cmake ninja-build pkg-config \
        qemu-system-arm qemu-utils python3-pil \
    && rm -rf /var/lib/apt/lists/*

# Buildroot should not run as root.
RUN useradd --create-home --uid 1000 builder

# Rust for samplepack (the oscairn package builds its static library with cargo): the release
# Oscairn's CMakeLists.txt pins (OSCAIRN_RUST_VERSION), with the aarch64 standard library for the
# image's target, installed with rustup in /opt/rust; the oscairn package finds cargo there.
ENV RUSTUP_HOME=/opt/rust/rustup CARGO_HOME=/opt/rust/cargo
RUN wget -qO /tmp/rustup-init.sh https://sh.rustup.rs \
    && sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain none --no-modify-path \
    && /opt/rust/cargo/bin/rustup toolchain install 1.100.0 --profile minimal --component clippy,rustfmt \
        --target aarch64-unknown-linux-gnu --target x86_64-unknown-linux-gnu \
    && rm /tmp/rustup-init.sh && chown -R builder: /opt/rust
