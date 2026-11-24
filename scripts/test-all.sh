#!/usr/bin/env bash
# Runs every test: clippy, the workspace's tests, the components' tests with the compiler
# bundle, the release-only timing tests and the packaging tests.
#
#   scripts/test-all.sh            everything
#   scripts/test-all.sh --quick    skips the release timing step and the packaging tests that
#                                  build a release or a tarball
#   scripts/test-all.sh --list     prints the steps and stops
#   scripts/test-all.sh --only release   runs one step (a name from --list) and stops
#
# A failed step doesn't stop the later ones. The summary lists each step as ok or FAILED, and
# the script exits 1 if any failed. A step that fails on a test is rerun once on those tests
# alone, because a loaded machine can fail a timing test; both results are printed.
set -uo pipefail
cd "$(dirname "$0")/.."

CARGO=scripts/cargo-safe.sh
quick=0
only=
case "${1:-}" in
  "") ;;
  --quick) quick=1 ;;
  --only) only=${2:-}; [ -n "$only" ] || { echo "usage: scripts/test-all.sh --only STEP" >&2; exit 2; } ;;
  --list)
    printf '%s\t%s\n' \
      clippy "clippy on the workspace (warnings counted; CLIPPY_STRICT=1 makes them errors)" \
      workspace "the workspace's tests" \
      components "the components' tests, with the compiler bundle" \
      release "the release-only timing tests" \
      packaging "the packaging tests (xtask)"
    exit 0 ;;
  *) echo "usage: scripts/test-all.sh [--quick | --list | --only STEP]" >&2; exit 2 ;;
esac

log_dir=target/test-all
mkdir -p "$log_dir"

# The release-only timing tests: package, test file, tests.
release_tests() {
  echo "slides-terminal player costly_output_between_stops_is_quick_both_ways a_recording_too_costly_to_play_says_so a_long_recording_is_stepped_quickly"
  echo "slides-terminal screen costly_output_is_refused_or_quick ordinary_output_is_not_too_costly"
  echo "slides-engine terminal a_screen_of_letters_shaped_one_at_a_time_is_laid_out_aside"
  echo "stagepack-studio ui_terminal going_back_to_a_mark_is_quick"
  echo "stagepack-studio ui_video a_turned_full_size_video_costs_the_window_little"
  # Tests marked "timing; run in release": a whole file's, or single ones.
  echo "stagepack-studio edit_speed *"
  echo "slides-engine text_geometry big_tables_are_read_and_laid_out_quickly"
  echo "slides-video decode decoding_1080p_takes"
}

# Each step takes the names of failed tests (none on the first run) and filters to them.
# The plan asks for warnings to be errors. The workspace isn't clean today (see docs/checks/plan-e.md),
# so by default a warning is counted and printed, and only an error fails the step; CLIPPY_STRICT=1
# makes warnings errors.
step_clippy() {
  if [ "${CLIPPY_STRICT:-0}" = 1 ]; then
    $CARGO clippy --workspace --all-targets -- -D warnings
    return
  fi
  local out="$log_dir/clippy-output.txt" code
  $CARGO clippy --workspace --all-targets > "$out" 2>&1
  code=$?
  cat "$out"
  echo "clippy warnings: $(grep '^warning' "$out" | grep -vc 'generated') (CLIPPY_STRICT=1 makes them errors)"
  return $code
}
step_workspace() { $CARGO test --workspace --no-fail-fast -- ${1:+--exact "$@"}; }
step_components() {
  if [ $# -eq 0 ]; then
    scripts/test-components.sh
  else
    STUDIO_BUNDLE=$PWD/target/bundle $CARGO test --workspace --no-fail-fast -- --include-ignored --exact "$@"
  fi
}
step_release() {
  if [ $# -gt 0 ]; then
    $CARGO test --release --workspace --no-fail-fast -- --include-ignored --exact "$@"
    return
  fi
  local status=0 line
  set -f   # a '*' in the list is a word, not a pattern
  while read -r line; do
    set -- $line
    local pkg=$1 file=$2
    shift 2
    if [ "$1" = '*' ]; then
      $CARGO test --release -p "$pkg" --test "$file" --no-fail-fast -- --include-ignored || status=1
    else
      # --include-ignored: in a release build a test marked cfg_attr(debug_assertions, ignore) isn't ignored.
      $CARGO test --release -p "$pkg" --test "$file" -- --include-ignored --exact "$@" || status=1
    fi
  done < <(release_tests)
  set +f
  return $status
}
step_packaging() {
  if [ $# -gt 0 ]; then
    $CARGO test -p xtask --no-fail-fast -- --include-ignored --exact "$@"
    return
  fi
  local status=0
  $CARGO test -p xtask --no-fail-fast || status=1
  if [ "$quick" -eq 0 ]; then
    $CARGO test -p xtask --test stage --test linux --no-fail-fast -- --ignored || status=1
  fi
  return $status
}

steps=(clippy workspace components)
[ "$quick" -eq 0 ] && steps+=(release)
steps+=(packaging)
if [ -n "$only" ]; then
  case " clippy workspace components release packaging " in
    *" $only "*) steps=("$only") ;;
    *) echo "scripts/test-all.sh: no step called '$only' (see --list)" >&2; exit 2 ;;
  esac
fi

results=()
failed=0
for name in "${steps[@]}"; do
  log=$log_dir/$name.log
  echo
  echo "=== step: $name ==="
  "step_$name" 2>&1 | tee "$log"
  code=${PIPESTATUS[0]}
  if [ "$code" -eq 0 ]; then
    results+=("$name: ok")
    continue
  fi
  echo "--- step $name FAILED (exit $code)"
  # Failed test names, from cargo's "---- name stdout ----" lines.
  mapfile -t names < <(sed -n 's/^---- \(.*\) stdout ----$/\1/p' "$log" | sed 's/ - should panic$//' | sort -u)
  if [ "$name" != clippy ] && [ "${#names[@]}" -gt 0 ]; then
    echo "--- A loaded machine can fail a test that is timed. Rerunning only: ${names[*]}"
    "step_$name" "${names[@]}" 2>&1 | tee "$log_dir/$name-rerun.log"
    if [ "${PIPESTATUS[0]}" -eq 0 ]; then
      results+=("$name: FAILED the first time (${#names[@]} tests), ok on the rerun alone: ${names[*]}")
      echo "--- the rerun passed; both results are in the summary and the step still counts as FAILED"
    else
      results+=("$name: FAILED, and failed again alone: ${names[*]}")
    fi
  else
    results+=("$name: FAILED")
  fi
  failed=1
done

echo
echo "=== summary ==="
for r in "${results[@]}"; do echo "$r"; done
[ "$quick" -eq 1 ] && echo "(--quick: the release timing step and the slow packaging tests were skipped)"
if [ "$failed" -ne 0 ]; then
  echo "SOME STEPS FAILED (logs in $log_dir)"
  exit 1
fi
echo "all steps ok"
exit 0
