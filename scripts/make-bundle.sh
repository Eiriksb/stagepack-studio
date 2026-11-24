#!/usr/bin/env bash
# Assembles the trimmed compiler bundle components are built with, and prints its size
# in MB: scripts/make-bundle.sh <dir>
set -euo pipefail
out=$(realpath -m "${1:?usage: make-bundle.sh <dir>}")
repo=$(cd "$(dirname "$0")/.." && pwd)
sysroot=$(rustc --print sysroot)
host=$(rustc -vV | sed -n 's/host: //p')
wasm=wasm32-unknown-unknown

# The folder is replaced, so make sure it's one of ours.
if [ -n "$(ls -A "$out" 2>/dev/null)" ] && [ ! -e "$out/bin/rustc" ]; then
  echo "make-bundle: $out has files in it and isn't a bundle, so it's been left alone" >&2
  exit 1
fi

rm -rf "$out"
mkdir -p "$out/bin" "$out/lib/rustlib/$host/bin" "$out/lib/rustlib/$wasm/lib" "$out/toolkit/deps" "$out/toolkit-src"
cp "$sysroot/bin/rustc" "$out/bin/"
cp -P "$sysroot"/lib/librustc_driver-*.so "$sysroot"/lib/libLLVM* "$out/lib/"
cp "$sysroot/lib/rustlib/$host/bin/rust-lld" "$out/lib/rustlib/$host/bin/"
cp "$sysroot/lib/rustlib/$wasm/lib/"*.rlib "$sysroot/lib/rustlib/$wasm/lib/"*.rmeta "$out/lib/rustlib/$wasm/lib/"

cd "$repo"
# A target folder of its own and made afresh, so that only one variant of each library
# is copied. STUDIO_BUNDLE_BUILD (inside target/) picks another, as the container build does.
build=${STUDIO_BUNDLE_BUILD:-target/bundle-build}
case "$build" in
  target/?*) case "$build" in *..*) echo "make-bundle: STUDIO_BUNDLE_BUILD must be a folder inside target/" >&2; exit 1 ;; esac ;;
  *) echo "make-bundle: STUDIO_BUNDLE_BUILD must be a folder inside target/" >&2; exit 1 ;;
esac
rm -rf "$build"
# The libraries' panic messages name their source files, and they end up in every build.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$cargo_home=/cargo"$'\x1f'"--remap-path-prefix=$repo=/studio"
scripts/cargo-safe.sh build --release -q -p toolkit --target "$wasm" --target-dir "$build"
cp "$build/$wasm/release/libtoolkit.rlib" "$out/toolkit/"
cp "$build/$wasm/release/deps/"*.rlib "$build/$wasm/release/deps/"*.rmeta "$out/toolkit/deps/"
cp "$build"/release/deps/libtoolkit_macros-*.so "$build"/release/deps/libserde_derive-*.so "$out/toolkit/deps/"

# The source, for a code editor, with a workspace standing in for ours.
cp -R crates/toolkit crates/toolkit-macros "$out/toolkit-src/"
cat > "$out/toolkit-src/Cargo.toml" <<'END'
[workspace]
resolver = "3"
members = ["toolkit", "toolkit-macros"]

[workspace.package]
edition = "2024"
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
END

# The guide for writing components, which ships with the app.
mkdir -p "$out/guide"
cp docs/guide/components.md "$out/guide/"

du -sm "$out" | cut -f1
