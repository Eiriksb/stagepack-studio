#!/usr/bin/env bash
# Builds the compiler bundle and runs the tests that need it: the ones marked "needs the
# compiler bundle", the AI tool's build with the owner's permission, and the reader's build
# for WebAssembly, which the Slidev add-on needs.
# (The timing tests and the packaging tests have their own steps in test-all.sh.)
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/make-bundle.sh target/bundle
export STUDIO_BUNDLE=$PWD/target/bundle
status=0
scripts/cargo-safe.sh test -p slides-components --test compile --test examples --no-fail-fast -- --ignored || status=1
scripts/cargo-safe.sh test -p stagepack-studio --test builds --no-fail-fast -- --ignored || status=1
scripts/cargo-safe.sh test -p stagepack-studio --test ai_tool --no-fail-fast || status=1
scripts/cargo-safe.sh test -p slides-format --test wasm_build --no-fail-fast -- --ignored || status=1
exit $status
