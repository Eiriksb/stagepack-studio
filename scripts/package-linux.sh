#!/usr/bin/env bash
# Builds the Linux package in an Ubuntu 22.04 container, so that it needs no glibc newer than 2.35
# and runs on Ubuntu 22.04 and Debian 12 or newer. Then checks every ELF file in it on the host.
#
#   scripts/package-linux.sh                 reuses the container's bundle (target/bundle-22.04) if it's there
#   scripts/package-linux.sh --bundle-fresh  makes the bundle again first
#
# Leaves target/package/stagepack-studio-<version>-linux-x86_64.tar.zst. Only target/ is written to:
# the container keeps its own folders there (*-22.04), so nothing the host built is mixed in.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"

fresh=0
case "${1:-}" in
  "") ;;
  --bundle-fresh) fresh=1 ;;
  *) echo "usage: scripts/package-linux.sh [--bundle-fresh]" >&2; exit 2 ;;
esac

image=stagepack-studio-build:22.04
volume=stagepack-studio-cargo
staged=target/package-stage-22.04
bundle=target/bundle-22.04

docker build -t "$image" -f packaging/linux/build.Dockerfile packaging/linux
# A new named volume belongs to root; the build runs as the caller.
docker run --rm -v "$volume:/c" ubuntu:22.04 chown "$(id -u):$(id -g)" /c

# Inside: the bundle, its proc-macros built here (never the host's target/bundle-build), the
# release builds in a target folder of their own, the staged folder and the tarball.
inside='set -euo pipefail
mkdir -p "$HOME"
export CARGO_TARGET_DIR=target/xtask-22.04
if [ "$FRESH" = 1 ] || [ ! -e "$BUNDLE/bin/rustc" ]; then
  STUDIO_BUNDLE_BUILD=target/bundle-build-22.04 bash scripts/make-bundle.sh "$BUNDLE"
fi
STUDIO_PACKAGE_TARGET=target/package-build-22.04 cargo run -q -p xtask --bin package -- stage "$STAGED" --bundle "$BUNDLE"
cargo run -q -p xtask --bin package -- linux --staged "$STAGED"'

docker run --rm --memory 18g --user "$(id -u):$(id -g)" \
  -v "$repo:/studio" -v "$volume:/cargo-cache" -w /studio \
  -e FRESH="$fresh" -e BUNDLE="$bundle" -e STAGED="$staged" \
  --name stagepack-studio-package \
  "$image" bash -c "$inside"

# On the host: every program and library in the package against the ceiling.
scripts/cargo-safe.sh run -q -p xtask --bin package -- glibc-check "$staged"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/app/Cargo.toml | head -n 1)
echo "$repo/target/package/stagepack-studio-$version-linux-x86_64.tar.zst"
