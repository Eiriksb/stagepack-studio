#!/usr/bin/env bash
# Builds the Linux package in an Ubuntu 22.04 container, so that it needs no glibc newer than 2.35
# and runs on Ubuntu 22.04 and Debian 12 or newer. Then checks every ELF file in it on the host.
#
#   scripts/package-linux.sh                 reuses the container's bundle (target/bundle-22.04) if it's there
#   scripts/package-linux.sh --bundle-fresh  makes the bundle again first
#
# Leaves target/package/stagepack-studio-<version>-linux-x86_64.tar.zst and, beside it, the marker
# target/package/.built-in-ubuntu-22.04 with the commit it was built from (test-all.sh tests the
# tarball on old systems only when the marker is there). Only target/ is written to: the repo is
# mounted read-only and target/ read-write, and the container keeps its own folders there (*-22.04),
# so nothing the host built is mixed in.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"

if [ "$(id -u)" = 0 ]; then
  echo "package-linux: don't run this as root or with sudo: what it leaves in target/ and in the cargo volume would belong to root, and the next run as yourself couldn't write there. Run it as yourself, with a Docker you can use without sudo (the docker group, or rootless Docker)." >&2
  exit 1
fi

fresh=0
case "${1:-}" in
  "") ;;
  --bundle-fresh) fresh=1 ;;
  *) echo "usage: scripts/package-linux.sh [--bundle-fresh]" >&2; exit 2 ;;
esac

image=stagepack-studio-build:22.04
volume=stagepack-studio-cargo
container=stagepack-studio-package
staged=target/package-stage-22.04
bundle=target/bundle-22.04
marker=target/package/.built-in-ubuntu-22.04

if ! security=$(docker info --format '{{.SecurityOptions}}' 2>/dev/null); then
  echo "package-linux: docker isn't usable here: 'docker info' failed (is the daemon running, and can you use it without sudo?)" >&2
  exit 1
fi
# Under rootless Docker the container's root is the caller on the host, and any other uid in the
# container is one the caller's files don't belong to. So there the build runs as root inside.
case "$security" in
  *name=rootless*) user=0:0 rootless=1 ;;
  *) user="$(id -u):$(id -g)" rootless=0 ;;
esac

# Until the new tarball passes glibc-check, nothing says target/package holds a container build.
mkdir -p target/package
rm -f "$marker"

docker build -t "$image" -f packaging/linux/build.Dockerfile packaging/linux
if [ "$rootless" = 0 ]; then
  # A new named volume belongs to root; the build runs as the caller.
  docker run --rm -v "$volume:/c" ubuntu:22.04 chown "$user" /c
fi
if [ "$fresh" = 0 ] && [ -e "$bundle/bin/rustc" ]; then
  echo "package-linux: reusing $bundle (use --bundle-fresh after a toolkit or toolchain change)"
fi

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

# --init gives the build a PID 1 that passes signals on to it, and Ctrl-C (or a closed terminal)
# stops our container rather than leaving it building. The run waits in the background so that
# the trap runs at once.
stop_build() {
  if kill -0 "$run" 2>/dev/null; then
    echo "package-linux: stopping the container $container" >&2
    docker stop -t 5 "$container" >/dev/null 2>&1 || true
  fi
  exit 130
}
docker run --rm --init -e TINI_KILL_PROCESS_GROUP=1 --memory 18g --user "$user" \
  -v "$repo:/studio:ro" -v "$repo/target:/studio/target" -v "$volume:/cargo-cache" -w /studio \
  -e FRESH="$fresh" -e BUNDLE="$bundle" -e STAGED="$staged" \
  --name "$container" \
  "$image" bash -c "$inside" &
run=$!
trap stop_build INT TERM HUP
wait "$run"
trap - INT TERM HUP

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/app/Cargo.toml | head -n 1)
tarball=target/package/stagepack-studio-$version-linux-x86_64.tar.zst

# On the host: every program and library in the package against the ceiling. A package that fails
# it is deleted, so that nothing picks it up.
if ! scripts/cargo-safe.sh run -q -p xtask --bin package -- glibc-check "$staged"; then
  rm -f "$tarball"
  echo "package-linux: glibc-check failed, so $tarball has been deleted" >&2
  exit 1
fi

# The marker: the commit, and "dirty" if the tree had changes the commit doesn't hold.
commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)
if [ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]; then
  commit="$commit dirty"
fi
echo "$commit" > "$marker"
echo "$repo/$tarball"
