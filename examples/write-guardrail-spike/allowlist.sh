#!/usr/bin/env bash
# Arm 3 of the WP-17 spike: **where does a real build, a real git operation and a
# real `fleet` invocation actually write?**
#
# The allowlist is the part of this package that can wedge a pane while it still
# looks healthy, so it is measured rather than reasoned about (building.md §4).
# Method: touch a reference file, run the command, then `find -newer` every
# directory the answer could plausibly be in. No sudo, no fs_usage, no guessing.
#
#     bash examples/write-guardrail-spike/allowlist.sh
#
# Free — no `claude`, no tokens. Everything lands in work/, which is gitignored.

set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="$HERE/work/allowlist"
REAL_HOME="$HOME"

rm -rf "$WORK"
mkdir -p "$WORK"

TARGET="$WORK/target-repo"           # stands in for the operator's repo
SHELL_DIR="$WORK/fleetor/_shell"     # stands in for ~/.fleetor/_shell
WT="$SHELL_DIR/worktrees/worker-1"   # the worker's own worktree
WHOME="$SHELL_DIR/home/worker-1"     # the Fence's private HOME (D-052)
mkdir -p "$SHELL_DIR/worktrees" "$WHOME"

# --- a real repo with a real cargo project ------------------------------------

mkdir -p "$TARGET/src"
cd "$TARGET" || exit 1
git init -q -b main .
# The empty `[workspace]` table keeps this throwaway crate out of FLEETOR's own
# workspace — without it cargo refuses to build anything here at all.
printf '[package]\nname = "spikey"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n\n[dependencies]\nitoa = "1"\n' > Cargo.toml
printf 'fn main() { println!("{}", itoa::Buffer::new().format(41)); }\n' > src/main.rs
git -c user.name=spike -c user.email=s@x add -A
git -c user.name=spike -c user.email=s@x commit -qm "the pinned tree"

# The exact command `fleet::ensure_worktree` runs.
git worktree add -q -B fleet/worker-1 "$WT"
printf '[user]\n\tname = fleet worker-1\n\temail = worker-1@fleetor.local\n' > "$WHOME/.gitconfig"

# --- the measurement ----------------------------------------------------------

# Every root an answer could be in. `$REAL_HOME/.cargo` and `.rustup` are the
# ones the plan guessed at and the spike is here to settle.
scan() {
  local ref="$1" label="$2"
  echo
  echo "### $label"
  for dir in "$WT" "$TARGET/.git" "$SHELL_DIR" "$WHOME" "$REAL_HOME/.cargo" "$REAL_HOME/.rustup"; do
    [ -d "$dir" ] || continue
    local n
    n=$(find "$dir" -newer "$ref" -type f 2>/dev/null | wc -l | tr -d ' ')
    local where="$dir"
    case "$dir" in
      "$WT") where="the worker's worktree            [IN allowlist]" ;;
      "$TARGET/.git") where="the target repo's .git           [OUT of a worker's allowlist]" ;;
      "$SHELL_DIR") where="~/.fleetor/_shell                [IN allowlist]" ;;
      "$WHOME") where="the worker's private HOME        [IN allowlist, it is under _shell]" ;;
      "$REAL_HOME/.cargo") where="the operator's ~/.cargo          [OUT of every allowlist]" ;;
      "$REAL_HOME/.rustup") where="the operator's ~/.rustup         [OUT of every allowlist]" ;;
    esac
    printf '  %-6s files written under %s\n' "$n" "$where"
    if [ "$n" != "0" ]; then
      find "$dir" -newer "$ref" -type f 2>/dev/null | sed "s|$WORK|\$WORK|; s|$REAL_HOME|~|" | head -4 | sed 's/^/           /'
    fi
  done
}

step() {
  local label="$1" home="$2" shellcmd="$3"
  local ref="$WORK/.ref"
  sleep 1
  touch "$ref"
  sleep 1
  echo
  echo "=== $label"
  echo "    HOME=$( [ "$home" = "$WHOME" ] && echo "the worker's private one" || echo "the operator's real one" )"
  echo "    \$ $shellcmd"
  env -u RUSTUP_HOME -u CARGO_HOME HOME="$home" PATH="$PATH" \
      FLEETOR_PANE=worker-1 FLEET_SOCKET="$SHELL_DIR/fleet.sock" \
      sh -c "$shellcmd" > "$WORK/$label.log" 2>&1
  echo "    exit $?  (log: work/allowlist/$label.log)"
  scan "$ref" "$label"
}

# 1. a worker's cargo build, under the Fence's private HOME
step "worker-cargo-build" "$WHOME" "cd '$WT' && cargo build"

# 2. the same build with the operator's real toolchain reachable — what a
#    *working* worker build writes, once the rustup breakage in 1 is fixed.
step "worker-cargo-build-real-toolchain" "$WHOME" \
  "cd '$WT' && RUSTUP_HOME='$REAL_HOME/.rustup' CARGO_HOME='$REAL_HOME/.cargo' cargo build"

# 3. the same build under the operator's real HOME — orch's posture
step "orch-cargo-build" "$REAL_HOME" "cd '$TARGET' && cargo build"

# 4. a worker's first commit — `fleet done`'s first step
step "worker-git-commit" "$WHOME" \
  "cd '$WT' && echo 'pub fn parse() {}' > parser.rs && git add parser.rs && git commit -qm 'worker-1: a parser'"

# 5. a peer's three-dot diff — the WP-06 review move
step "worker-git-diff" "$WHOME" "cd '$WT' && git diff HEAD...fleet/worker-1 && git log --oneline -3"

# 6. the `fleet` CLI itself
FLEET="$ROOT/target/debug/fleet"
if [ -x "$FLEET" ]; then
  step "fleet-whoami" "$WHOME" "'$FLEET' whoami"
else
  echo; echo "=== fleet: $FLEET not built — run cargo build -p fleetor-cli --bin fleet"
fi

echo
echo "done. logs in $WORK"
