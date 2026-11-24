# The image the Linux package is built in: scripts/package-linux.sh builds and runs it.
# Ubuntu 22.04's glibc is 2.35, so what is built here also runs on Ubuntu 22.04 and Debian 12 (2.36).
FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl build-essential pkg-config cmake nasm zstd binutils \
        libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libwayland-dev libvulkan-dev \
        libfontconfig-dev libfreetype-dev \
    && rm -rf /var/lib/apt/lists/*

# The toolchain lives in /opt/rust, readable by the uid the build runs as.
ENV RUSTUP_HOME=/opt/rust/rustup CARGO_HOME=/opt/rust/cargo
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --default-toolchain 1.98.0 --profile minimal \
    && /opt/rust/cargo/bin/rustup target add wasm32-unknown-unknown \
    && chmod -R a+rX /opt/rust

# Cargo's registry is the named volume mounted at /cargo-cache; the home is a scratch folder.
ENV PATH=/opt/rust/cargo/bin:$PATH CARGO_HOME=/cargo-cache HOME=/tmp/home
