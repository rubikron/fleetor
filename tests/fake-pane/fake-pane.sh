#!/usr/bin/env bash
# A pane that is not `claude`. Stands in for one wherever a test needs a real
# pty with a real process on the far end and no tokens spent: it announces
# itself, then echoes every submitted line back with a marker.
#
# Selected by setting FLEETOR_PANE_CMD to this script's path; `src-tauri/spawn.rs`
# drops the `claude` flags when it is set, so this needs no argument handling.
echo "fake-pane ${FLEETOR_PANE:-unnamed} ready"
while IFS= read -r line; do
  echo "echo: $line"
done
