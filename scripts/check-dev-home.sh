#!/usr/bin/env bash
# Starts `stagepack serve` on the fixture, waits for ready, checks Studio answers, stops it.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/dev.env"
HOME_DIR="$ROOT/target/dev-home"
TMP="$(mktemp -d)"
# Serve also claims the talk in the per-user Stagepack it finds there: a check leaves that one alone.
export XDG_DATA_HOME="$TMP/data"
OUT="$TMP/out.jsonl"
: > "$OUT"
mkfifo "$TMP/in"
cleanup() { [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null || true; exec 3>&- 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
fail() { echo "dev home check: FAILED: $1" >&2; echo "events seen:" >&2; cat "$OUT" >&2; exit 1; }

"$HOME_DIR/stagepack" serve --json --home "$HOME_DIR" \
  --addon "$HOME_DIR/runtimes/53/node_modules/slidev-addon-studio" \
  "$ROOT/target/fixtures/hello.sptx" < "$TMP/in" > "$OUT" 2>"$TMP/err" &
PID=$!
exec 3> "$TMP/in"

BASE=""
for _ in $(seq 1 90); do
  if grep -q '"event":"error"' "$OUT"; then fail "serve reported an error"; fi
  BASE="$(grep '"event":"ready"' "$OUT" | head -1 | jq -r .baseUrl 2>/dev/null || true)"
  [ -n "$BASE" ] && break
  kill -0 "$PID" 2>/dev/null || fail "serve exited before ready"
  sleep 1
done
[ -n "$BASE" ] || fail "no ready within 90 s"

BODY="$TMP/catalog"
CODE="$(curl -s -o "$BODY" -w '%{http_code}' "$BASE/@studio/catalog" || true)"
[ "$CODE" = "200" ] || fail "GET /@studio/catalog returned HTTP $CODE"
jq -e 'has("components")' "$BODY" >/dev/null 2>&1 || fail "catalog is not JSON with components"

exec 3>&-
for _ in $(seq 1 30); do
  grep -q '"event":"stopped"' "$OUT" && break
  sleep 1
done
grep -q '"event":"stopped"' "$OUT" || fail "no stopped after closing stdin"
STATUS=0; wait "$PID" || STATUS=$?
PID=""
[ "$STATUS" = 0 ] || fail "serve exited with $STATUS"
echo "dev home check: ok"
