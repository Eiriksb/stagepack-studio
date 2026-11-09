#!/usr/bin/env bash
# Checks a talk the app saved, after the Linux check set the Box's X to 140:
#   1. slides.md holds the new position;
#   2. nothing of the editor was saved into it;
#   3. its folder still packs, so the talk is still a plain project;
#   4. plain Stagepack, with no editor addon, serves it.
#
#   scripts/check-saved.sh "path/to/my talk.sptx"
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/dev.env"

[ "$#" -eq 1 ] || { echo "usage: check-saved.sh FILE.sptx" >&2; exit 2; }
[ -f "$1" ] || { echo "saved talk check: FAILED: no such file: $1" >&2; exit 1; }
FILE="$(realpath -- "$1")"

DEV_HOME="${STUDIO_DEV_HOME:-target/dev-home}"
case "$DEV_HOME" in /*) ;; *) DEV_HOME="$ROOT/$DEV_HOME" ;; esac
PLAIN_HOME="$STAGEPACK_SRC/dist/linux"
TIME_LIMIT=90
EXPECTED='v-drag="[140,100,200,120]"'

TMP="$(mktemp -d)"
OUT="$TMP/events.jsonl"
PID=""

# Ends serve however the script ends: the end of stdin is the clean way, and
# serve's Slidev dies with it (Stagepack starts it with a parent-death signal).
stop_serve() {
  [ -n "$PID" ] || return 0
  exec 3>&-
  local signal
  for signal in 0 TERM KILL; do
    [ "$signal" = 0 ] || kill "-$signal" "$PID" 2>/dev/null || true
    for _ in $(seq 1 50); do
      kill -0 "$PID" 2>/dev/null || break 2
      sleep 0.2
    done
  done
  wait "$PID" 2>/dev/null || true
  PID=""
}
trap 'stop_serve; rm -rf "$TMP"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

fail() {
  echo "saved talk check: FAILED: $1" >&2
  if [ -s "$OUT" ]; then
    echo "events seen:" >&2
    cat "$OUT" >&2
  fi
  exit 1
}

# A copy under a plain name: unzip reads [ ] * ? in an archive's name as a pattern.
cp -- "$FILE" "$TMP/talk.sptx"
unzip -q "$TMP/talk.sptx" -d "$TMP/talk" || fail "not a readable zip: $FILE"
[ -f "$TMP/talk/slides.md" ] || fail "the talk has no slides.md"

grep -qF -- "$EXPECTED" "$TMP/talk/slides.md" ||
  fail "slides.md does not contain $EXPECTED"

EDITOR_FILES="$(unzip -Z1 "$TMP/talk.sptx" | grep -E 'studio|addons-preload|\.slidev/' || true)"
[ -z "$EDITOR_FILES" ] || fail "the editor's files were saved into the talk: $EDITOR_FILES"
if grep -qF -- 'slidev-addon-studio' "$TMP/talk/slides.md"; then
  fail "slides.md names slidev-addon-studio"
fi

# pack writes its own manifest and refuses a folder that already has one.
rm -f -- "$TMP/talk/stagepack.json"
"$DEV_HOME/stagepack" pack "$TMP/talk" -o "$TMP/repacked.sptx" > "$TMP/pack.log" 2>&1 ||
  fail "stagepack pack failed: $(cat "$TMP/pack.log")"

# Started first: opening the pipe for writing waits until serve has it open.
: > "$OUT"
mkfifo "$TMP/in"
"$PLAIN_HOME/stagepack" serve --json --home "$PLAIN_HOME" "$FILE" \
  < "$TMP/in" > "$OUT" 2> "$TMP/err" &
PID=$!
exec 3> "$TMP/in"

DEADLINE=$((SECONDS + TIME_LIMIT))
left() { echo $((DEADLINE - SECONDS)); }

BASE=""
while [ "$(left)" -gt 0 ]; do
  for event in busy confirm error; do
    grep -q "\"event\":\"$event\"" "$OUT" && fail "plain Stagepack said $event"
  done
  BASE="$(grep '"event":"ready"' "$OUT" | head -1 | jq -r .baseUrl 2>/dev/null || true)"
  [ -n "$BASE" ] && break
  kill -0 "$PID" 2>/dev/null || fail "plain Stagepack exited before ready: $(cat "$TMP/err")"
  sleep 0.5
done
[ -n "$BASE" ] || fail "plain Stagepack was not ready within $TIME_LIMIT s"

# Plus one: curl reads --max-time 0 as no limit.
CODE="$(curl -s --noproxy '*' --max-time "$(($(left) + 1))" -o /dev/null -w '%{http_code}' "$BASE/" || true)"
[ "$CODE" = "200" ] || fail "GET $BASE/ returned HTTP $CODE"

exec 3>&-
while ! grep -q '"event":"stopped"' "$OUT"; do
  [ "$(left)" -gt 0 ] || fail "no stopped within $TIME_LIMIT s of starting serve"
  kill -0 "$PID" 2>/dev/null || { grep -q '"event":"stopped"' "$OUT" && break; fail "serve exited without stopped"; }
  sleep 0.2
done
while kill -0 "$PID" 2>/dev/null; do
  [ "$(left)" -gt 0 ] || fail "serve did not exit after stopped"
  sleep 0.2
done
STATUS=0
wait "$PID" || STATUS=$?
PID=""
[ "$STATUS" = 0 ] || fail "serve exited with $STATUS"

echo "saved talk check: ok"
