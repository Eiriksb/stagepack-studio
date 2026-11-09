#!/usr/bin/env bash
# Runs cargo in its own memory-limited systemd scope. A GPUI build can use more
# memory than the desktop can spare, and systemd-oomd kills the whole scope it
# finds responsible: without this, that's the app or terminal cargo was started
# from. With it, only the build dies.
#
#   scripts/cargo-safe.sh test --workspace
#   CARGO_BUILD_JOBS=3 scripts/cargo-safe.sh build
set -euo pipefail

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}"

if systemd-run --user --scope --collect -q -- true 2>/dev/null; then
  exec systemd-run --user --scope --collect -q \
    -p MemoryHigh=14G -p MemoryMax=18G -p MemorySwapMax=2G -- cargo "$@"
fi
echo "cargo-safe: no systemd user session, running cargo without a memory limit" >&2
exec cargo "$@"
