#!/usr/bin/env bash
# Builds the compiler bundle and runs the tests that need it.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/make-bundle.sh target/bundle
STUDIO_BUNDLE=$PWD/target/bundle scripts/cargo-safe.sh test --workspace -- --ignored
