#!/usr/bin/env bash
# Issue 10 spike: can a codex worker commit in a linked worktree, and which
# writable_roots make it possible?
#
#     bash examples/codex-commit-spike/commit.sh
#
# Free: every arm is `codex sandbox`, the real seatbelt with no model.
# PASS means the operation took effect on disk; FAIL means it did not.
# The fixture lives in target/issue10-spike and is removed at the end (KEEP=1 keeps it).

set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
FIX="$ROOT/target/issue10-spike"
CX="${CODEX:-/opt/homebrew/bin/codex}"   # not the cmux shim on PATH
REPO="$FIX/repo"
WT="$FIX/wt"
GIT="$REPO/.git"
LOG="$FIX/log"

git -C "$ROOT" check-ignore -q target/issue10-spike || { echo "target/ is not gitignored"; exit 1; }

ROOTS_B="[\"$GIT\"]"
ROOTS_B2="[\"$GIT\",\"$GIT/worktrees/wt\"]"
ROOTS_C="[\"$GIT/worktrees/wt\",\"$GIT/objects\",\"$GIT/refs\",\"$GIT/logs\"]"
COMMIT='echo x > f.txt && git add -A && git commit -qm probe'

fresh() {
  rm -rf "$FIX"
  mkdir -p "$REPO" "$FIX/codex-home"
  git -C "$REPO" init -q -b main .
  git -C "$REPO" config user.name spike
  git -C "$REPO" config user.email s@x
  echo base > "$REPO/README.md"
  git -C "$REPO" add -A
  git -C "$REPO" commit -qm base
  git -C "$REPO" worktree add -q -b fleet/probe ../wt
}

# sbx <cwd> <writable_roots toml, or ""> <shell command>
sbx() {
  local cwd="$1" roots="$2" cmd="$3"
  local args=(sandbox
    -c sandbox_mode=workspace-write
    -c approval_policy=never
    -c sandbox_workspace_write.network_access=true
    -c sandbox_workspace_write.exclude_slash_tmp=true
    -c sandbox_workspace_write.exclude_tmpdir_env_var=true)
  [ -n "$roots" ] && args+=(-c "sandbox_workspace_write.writable_roots=$roots")
  ( cd "$cwd" && CODEX_HOME="$FIX/codex-home" "$CX" "${args[@]}" -- sh -c "$cmd" ) > "$LOG" 2>&1
}

evidence() { grep -m1 -iE 'fatal|error|not permitted|denied|unable|cannot' "$LOG" | sed "s|$FIX|\$FIX|g"; }

# report <arm> <0 if it took effect>
report() {
  if [ "$2" = 0 ]; then printf 'PASS  %s\n' "$1"; else printf 'FAIL  %s\n      | %s\n' "$1" "$(evidence)"; fi
}

committed() { # <dir> <subject>
  [ "$(git -C "$1" log -1 --format=%s)" = "$2" ] && [ -z "$(git -C "$1" status --porcelain)" ]
}

echo "# $("$CX" --version) — $(date '+%Y-%m-%d %H:%M')"

fresh; sbx "$WT" "" "$COMMIT"
committed "$WT" probe; report "A  wt, no writable_roots: commit" $?

fresh; sbx "$WT" "$ROOTS_B" "$COMMIT"
committed "$WT" probe; report "B  wt, roots=[.git]: commit" $?

fresh; sbx "$WT" "$ROOTS_B2" "$COMMIT"
committed "$WT" probe; report "B2 wt, roots=[.git, worktrees/wt]: commit" $?

fresh; sbx "$WT" "$ROOTS_C" "$COMMIT"
committed "$WT" probe; report "C  wt, roots=[worktrees/wt, objects, refs, logs]: commit" $?

# C with one root left out, to name the minimal set.
for drop in worktrees/wt objects refs logs; do
  roots="["
  for keep in worktrees/wt objects refs logs; do
    [ "$keep" = "$drop" ] || roots="$roots\"$GIT/$keep\","
  done
  roots="${roots%,}]"
  fresh; sbx "$WT" "$roots" "$COMMIT"
  committed "$WT" probe; report "C- wt, C without $drop: commit" $?
done

for set in B2 C; do
  [ "$set" = B2 ] && roots="$ROOTS_B2" || roots="$ROOTS_C"

  fresh; sbx "$WT" "$roots" "$COMMIT"
  sbx "$WT" "$roots" 'git commit -q --amend -m x'
  committed "$WT" x; report "D/$set  commit --amend" $?

  sbx "$WT" "$roots" 'git switch -q -c other'
  [ "$(git -C "$WT" branch --show-current)" = other ]; report "D/$set  switch -c other" $?

  sbx "$WT" "$roots" 'git gc -q'; rc=$?
  [ $rc = 0 ] && ls "$GIT"/objects/pack/*.pack >/dev/null 2>&1; report "D/$set  gc" $?

  sbx "$WT" "$roots" 'echo y >> f.txt && git stash -q && git stash pop -q'; rc=$?
  [ $rc = 0 ] && [ -n "$(git -C "$WT" status --porcelain)" ] && [ -z "$(git -C "$WT" stash list)" ]
  report "D/$set  stash && stash pop" $?
done

for set in B B2 C; do
  case "$set" in B) roots="$ROOTS_B" ;; B2) roots="$ROOTS_B2" ;; C) roots="$ROOTS_C" ;; esac

  fresh; sbx "$WT" "$roots" "printf '#!/bin/sh\n' > '$GIT/hooks/pre-commit'"
  [ -f "$GIT/hooks/pre-commit" ]; report "E/$set  write .git/hooks/pre-commit" $?

  sbx "$WT" "$roots" "echo '# probe' >> '$GIT/config'"
  grep -q '# probe' "$GIT/config"; report "E/$set  write .git/config" $?

  sbx "$WT" "$roots" "echo x >> '$REPO/README.md'"
  [ "$(cat "$REPO/README.md")" != base ]; report "G/$set  write repo/README.md" $?
done

fresh; sbx "$REPO" "" "$COMMIT"
committed "$REPO" probe; report "F  cwd=repo, no writable_roots: commit" $?

merge_arm() { # <label> <roots>
  fresh
  ( cd "$WT" && echo w > w.txt && git add -A && git commit -qm from-worker )
  sbx "$REPO" "$2" 'git merge -q fleet/probe'
  [ -f "$REPO/w.txt" ] && [ "$(git -C "$REPO" log -1 --format=%s)" = from-worker ]
  report "$1" $?
}
merge_arm "F  cwd=repo, no writable_roots: merge fleet/probe" ""

fresh; sbx "$REPO" "$ROOTS_B" "$COMMIT"
committed "$REPO" probe; report "F2 cwd=repo, roots=[.git]: commit" $?
merge_arm "F2 cwd=repo, roots=[.git]: merge fleet/probe" "$ROOTS_B"

# H: the seed writes config.toml, not -c. Does `codex sandbox` read roots from the file?
fresh
printf '[sandbox_workspace_write]\nwritable_roots = %s\n' "$ROOTS_C" > "$FIX/codex-home/config.toml"
sbx "$WT" "" "$COMMIT"
committed "$WT" probe; report "H  wt, C's roots in CODEX_HOME/config.toml only: commit" $?

[ "${KEEP:-0}" = 1 ] || rm -rf "$FIX"
