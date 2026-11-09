#!/usr/bin/env bash
# Stand-in for `stagepack serve --json`: prints the lines of $FAKE_SERVE_SCRIPT,
# records stdin lines to $FAKE_SERVE_STDIN and its arguments to $FAKE_SERVE_ARGS,
# and on stdin EOF prints a stopped event and exits with $FAKE_SERVE_EXIT.
if [ -n "$FAKE_SERVE_ARGS" ]; then
  printf '%s\n' "$@" > "$FAKE_SERVE_ARGS"
fi
if [ -n "$FAKE_SERVE_SCRIPT" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    printf '%s\n' "$line"
  done < "$FAKE_SERVE_SCRIPT"
fi
while IFS= read -r line; do
  if [ -n "$FAKE_SERVE_STDIN" ]; then
    printf '%s\n' "$line" >> "$FAKE_SERVE_STDIN"
  fi
done
echo '{"event":"stopped"}'
exit "${FAKE_SERVE_EXIT:-0}"
