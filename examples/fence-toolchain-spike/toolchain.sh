#!/usr/bin/env bash
# The D-069 spike: **can a worker under the Fence build Rust, and where does the
# build write?**
#
# WP-17 open question 1 recorded that a worker cannot run `cargo build`: the
# Fence (D-052) gives it a private HOME, rustup resolves `$HOME/.rustup`, finds
# no toolchains, and gives up. That measurement — write-guardrail-notes.md §3.4 —
# ran every arm with `PATH="$PATH"`, the operator's *login* PATH, which carries
# `~/.cargo/bin`. It therefore never asked whether a worker can find `cargo` at
# all, and the answer to that question changes the shape of the fix.
#
# So the rule every arm here obeys: **an explicitly stated PATH, and
# `-u CARGO_HOME -u RUSTUP_HOME`, never `PATH="$PATH"`.**
#
#     bash examples/fence-toolchain-spike/toolchain.sh
#
# Free — no `claude`, no tokens. Everything lands in work/, which is gitignored.
#
# **This spike writes outside work/ on purpose, and cleans up after itself.** Two
# arms have to, because where a write lands is the question:
#   - arm 6b downloads a whole uninstalled toolchain (~1.2 GB, 35,981 files) into
#     the operator's real ~/.rustup — that IS the finding;
#   - arm 7b installs a binary into the operator's real ~/.cargo/bin.
# Both are undone at the end. Budget ~2.5 GB of transient disk and a few minutes,
# and check the CLEANUP section's own report before trusting that it worked.

set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="$HERE/work"
REAL_HOME="$HOME"

rm -rf "$WORK"
mkdir -p "$WORK"

TARGET="$WORK/target-repo"            # stands in for the operator's repo
SHELL_DIR="$WORK/fleetor/_shell"      # stands in for ~/.fleetor/_shell
WT="$SHELL_DIR/worktrees/worker-1"    # the worker's own worktree
WT2="$SHELL_DIR/worktrees/worker-2"   # a second worker, for arm 8
WHOME="$SHELL_DIR/home/worker-1"      # the Fence's private HOME (D-052)
WHOME2="$SHELL_DIR/home/worker-2"
FCARGO="$SHELL_DIR/cargo"             # the PROPOSED fleet-owned CARGO_HOME
FRUSTUP="$SHELL_DIR/rustup"           # arm 9's mirrored RUSTUP_HOME
mkdir -p "$SHELL_DIR/worktrees" "$WHOME" "$WHOME2"

# --- the PATHs under test -----------------------------------------------------
#
# `worker_augmented_path()` (src-tauri/src/spawn.rs:303) builds
# `{fleet-bin}:/opt/homebrew/bin:/usr/local/bin:{inherited}`. The *inherited*
# tail is whatever launched the app, and that is the variable §3.4 held fixed by
# accident. Both real values are reproduced here.

FLEET_BIN_DIR="$ROOT/target/debug"
# What a terminal launch (`npm run tauri dev`) inherits: the operator's login PATH.
WPATH_TERM="$FLEET_BIN_DIR:/opt/homebrew/bin:/usr/local/bin:$PATH"
# What a Finder launch inherits: launchd's default, since `launchctl getenv PATH`
# is empty on this machine. Arm 1b measures this rather than assuming it.
LAUNCHD_DEFAULT="/usr/bin:/bin:/usr/sbin:/sbin"
WPATH_GUI="$FLEET_BIN_DIR:/opt/homebrew/bin:/usr/local/bin:$LAUNCHD_DEFAULT"
# The proposal: one extra rung, the fleet's own cargo bin, no operator-HOME rung.
WPATH_PROXY="$FLEET_BIN_DIR:$FCARGO/bin:/opt/homebrew/bin:/usr/local/bin:$LAUNCHD_DEFAULT"
# Option A's PATH: the operator's own cargo bin, the rung D-052 would have to
# put back. Shown with the GUI tail so it is the only thing that differs.
WPATH_OPERATOR="$FLEET_BIN_DIR:$REAL_HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$LAUNCHD_DEFAULT"

REAL_RUSTUP="$REAL_HOME/.cargo/bin/rustup"

# --- a real repo with a real cargo project ------------------------------------

mkdir -p "$TARGET/src"
cd "$TARGET" || exit 1
git init -q -b main .
# The empty `[workspace]` table keeps this throwaway crate out of FLEETOR's own
# workspace. `itoa` is a real crates.io dependency, so a build has to reach the
# registry — the part that writes into CARGO_HOME.
printf '[package]\nname = "spikey"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n\n[dependencies]\nitoa = "1"\n' > Cargo.toml
printf 'fn main() { println!("{}", itoa::Buffer::new().format(41)); }\n' > src/main.rs
git -c user.name=spike -c user.email=s@x add -A
git -c user.name=spike -c user.email=s@x commit -qm "the pinned tree"

# The exact commands `fleet::ensure_worktree` runs.
git worktree add -q -B fleet/worker-1 "$WT"
git worktree add -q -B fleet/worker-2 "$WT2"
printf '[user]\n\tname = fleet worker-1\n\temail = worker-1@fleetor.local\n' > "$WHOME/.gitconfig"
printf '[user]\n\tname = fleet worker-2\n\temail = worker-2@fleetor.local\n' > "$WHOME2/.gitconfig"

# --- the measurement ----------------------------------------------------------

scan() {
  local ref="$1"
  for dir in "$WT" "$TARGET/.git" "$WHOME" "$FCARGO" "$FRUSTUP" \
             "$REAL_HOME/.cargo" "$REAL_HOME/.rustup"; do
    [ -d "$dir" ] || continue
    local n where
    n=$(find "$dir" -newer "$ref" -type f 2>/dev/null | wc -l | tr -d ' ')
    case "$dir" in
      "$WT")               where="the worker's worktree          [IN allowlist]" ;;
      "$TARGET/.git")      where="the target repo's .git         [OUT of a worker's allowlist]" ;;
      "$WHOME")            where="the worker's private HOME      [IN allowlist, under _shell]" ;;
      "$FCARGO")           where="the FLEET's cargo home         [IN allowlist, under _shell]" ;;
      "$FRUSTUP")          where="the FLEET's rustup home        [IN allowlist, under _shell]" ;;
      "$REAL_HOME/.cargo") where="the operator's ~/.cargo        [OUT — and on their login PATH]" ;;
      "$REAL_HOME/.rustup")where="the operator's ~/.rustup       [OUT of every allowlist]" ;;
    esac
    printf '  %-6s %s\n' "$n" "$where"
    if [ "$n" != "0" ]; then
      find "$dir" -newer "$ref" -type f 2>/dev/null \
        | sed "s|$WORK|\$WORK|; s|$REAL_HOME|~|" | head -4 | sed 's/^/           /'
    fi
  done
}

# step <label> <home> <path> <extra-env, one string, may be empty> <shell command>
#
# `extra` is one space-separated string rather than an array on purpose: macOS
# ships bash 3.2, where an empty array under `set -u` is an unbound variable.
# No path in this spike contains a space, so the unquoted expansion is safe.
step() {
  local label="$1" home="$2" path="$3" extra="$4" shellcmd="$5"
  local ref="$WORK/.ref"
  sleep 1; touch "$ref"; sleep 1
  echo
  echo "=== $label"
  echo "    HOME=$( [ "$home" = "$REAL_HOME" ] && echo "the operator's real one" || echo "$( echo "$home" | sed "s|$WORK|\$WORK|" )" )"
  echo "    PATH=$( echo "$path" | sed "s|$WORK|\$WORK|; s|$REAL_HOME|~|" )"
  [ -n "$extra" ] && echo "    env: $( echo "$extra" | sed "s|$WORK|\$WORK|g; s|$REAL_HOME|~|g" )"
  echo "    \$ $( echo "$shellcmd" | sed "s|$WORK|\$WORK|g; s|$REAL_HOME|~|g" )"
  env -u RUSTUP_HOME -u CARGO_HOME -u RUSTUP_TOOLCHAIN \
      HOME="$home" PATH="$path" \
      FLEETOR_PANE=worker-1 FLEET_SOCKET="$SHELL_DIR/fleet.sock" \
      $extra \
      sh -c "$shellcmd" > "$WORK/$label.log" 2>&1
  local rc=$?
  echo "    exit $rc  (log: work/$label.log)"
  [ -s "$WORK/$label.log" ] && tail -3 "$WORK/$label.log" | sed "s|$REAL_HOME|~|g" | sed 's/^/    | /'
  scan "$ref"
}

echo "############################################################"
echo "# D-069 spike — the Fence and the Rust toolchain"
echo "# $(date '+%Y-%m-%d %H:%M')"
echo "############################################################"

# =============================================================================
# ARM 1 — can a worker find cargo at all? The question §3.4 could not ask.
# =============================================================================
echo
echo "=== 1 · can-a-worker-find-cargo"
for pair in "WPATH_TERM:$WPATH_TERM" "WPATH_GUI:$WPATH_GUI"; do
  name="${pair%%:*}"; p="${pair#*:}"
  echo "  --- $name"
  for tool in cargo rustup rustc cc git; do
    found=$(env -i PATH="$p" HOME="$WHOME" sh -c "command -v $tool" 2>/dev/null)
    printf '      %-8s %s\n' "$tool" "$( [ -n "$found" ] && echo "$found" | sed "s|$REAL_HOME|~|" || echo "NOT FOUND" )"
  done
done
echo "  --- do the two non-operator rungs the Fence supplies hold a cargo?"
for d in /opt/homebrew/bin /usr/local/bin; do
  printf '      %-20s %s\n' "$d/cargo" "$( [ -e "$d/cargo" ] && echo present || echo ABSENT )"
done

# =============================================================================
# ARM 1b — what a Finder-launched app actually inherits.
#
# NOT measured with `ps eww | tr ' ' '\n' | grep ^PATH=`: this machine's login
# PATH contains `/Applications/VMware Fusion.app/...`, a rung with a SPACE in it,
# and that splitter truncates PATH there — losing every rung after it, including
# `~/.cargo/bin`. Measured instead by launching a real bundle through launchd,
# which is the thing being asked about.
# =============================================================================
echo
echo "=== 1b · what-the-real-app-inherited"
APP="$WORK/PathProbe.app"
mkdir -p "$APP/Contents/MacOS"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>probe</string>
  <key>CFBundleIdentifier</key><string>local.fleetor.pathprobe</string>
  <key>CFBundleName</key><string>PathProbe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
cat > "$APP/Contents/MacOS/probe" <<PROBE
#!/bin/sh
{ echo "PATH=\$PATH"
  echo "cargo=\$(command -v cargo || echo NOT-FOUND)"
  echo "SENTINEL=\${FLEETOR_SPIKE_SENTINEL:-absent}"; } > "$WORK/gui-env.txt"
PROBE
chmod +x "$APP/Contents/MacOS/probe"

# The control, and it decides whether this arm means anything. `open` may hand
# the app the *calling shell's* environment rather than launchd's; if it does,
# this probe measures my terminal, not Finder. A sentinel variable and a
# deliberately mangled PATH settle it: if either shows up in the probe's output,
# `open` forwarded my environment and the arm is inconclusive.
rm -f "$WORK/gui-env.txt"
FLEETOR_SPIKE_SENTINEL=leaked PATH="/fleetor-spike-sentinel-rung:$PATH" open "$APP" 2>/dev/null
for _ in 1 2 3 4 5 6 7 8 9 10; do [ -f "$WORK/gui-env.txt" ] && break; sleep 1; done
if [ ! -f "$WORK/gui-env.txt" ]; then
  echo "  FAILED to launch the probe bundle — treat WPATH_GUI as unconfirmed."
else
  sentinel=$(grep '^SENTINEL=' "$WORK/gui-env.txt" | sed 's/^SENTINEL=//')
  if [ "$sentinel" = "leaked" ] || grep -q 'fleetor-spike-sentinel-rung' "$WORK/gui-env.txt"; then
    echo "  INCONCLUSIVE: \`open\` forwarded the calling shell's environment"
    echo "  (sentinel=$sentinel). This probe measured a terminal, not Finder —"
    echo "  so WPATH_GUI stays an inference and the arms below use it as one."
  else
    echo "  CONFIRMED: the probe saw neither the sentinel nor the mangled rung, so"
    echo "  this is launchd's own environment — what a Finder launch inherits."
  fi
  echo "  the bundle saw:"
  sed "s|$REAL_HOME|~|g" "$WORK/gui-env.txt" | cut -c1-160 | sed 's/^/      /'
  echo "  PATH rungs:"
  grep '^PATH=' "$WORK/gui-env.txt" | sed 's/^PATH=//' | tr ':' '\n' \
    | sed "s|$REAL_HOME|~|" | sed 's/^/        /'
fi
echo "  for contrast, launchctl getenv PATH: '$(launchctl getenv PATH)'"

# =============================================================================
# ARM 2 — the baseline, with the confound removed.
# =============================================================================
step "2-baseline-gui-path" "$WHOME" "$WPATH_GUI" "" \
  "cd '$WT' && cargo build"

step "2b-baseline-term-path" "$WHOME" "$WPATH_TERM" "" \
  "cd '$WT' && cargo build"

# =============================================================================
# ARM 3 — Option A: both homes pointed at the operator's real ones. The control,
# and the shape allowlist.sh:97 proved. Note it needs an operator-HOME PATH rung
# too, which that spike supplied by accident.
# =============================================================================
step "3-option-a-both-real" "$WHOME" "$WPATH_OPERATOR" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$REAL_HOME/.cargo" \
  "cd '$WT' && cargo build"

# =============================================================================
# ARM 4 — does a RELOCATED CARGO_HOME work at all? Load-bearing for Option B.
# cargo still found through the operator rung, so this isolates the one variable.
# =============================================================================
step "4-option-b-relocated-cargo-home" "$WHOME" "$WPATH_OPERATOR" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$FCARGO" \
  "cd '$WT' && cargo build"

# =============================================================================
# ARM 5 — does rustup DISPATCH through a proxy outside its own install dir?
# The other load-bearing question. No operator rung on PATH at all.
# =============================================================================
mkdir -p "$FCARGO/bin"
ln -sf "$REAL_RUSTUP" "$FCARGO/bin/cargo"
step "5a-seeded-proxy-cargo-only" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$FCARGO" \
  "cd '$WT' && cargo build && cargo --version"

for shim in rustc rustup rustdoc cargo-fmt cargo-clippy rustfmt clippy-driver; do
  ln -sf "$REAL_RUSTUP" "$FCARGO/bin/$shim"
done
step "5b-seeded-proxy-full-shim-set" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$FCARGO" \
  "cd '$WT' && cargo clean -q; cargo build && cargo --version && rustc --version && cargo fmt --check; echo fmt=\$?"

# =============================================================================
# ARM 6 — a rust-toolchain.toml. Does a pin write into the operator's ~/.rustup?
# =============================================================================
printf '[toolchain]\nchannel = "stable"\n' > "$WT/rust-toolchain.toml"
step "6a-toolchain-file-installed-channel" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$FCARGO" \
  "cd '$WT' && cargo build"

# An UNINSTALLED channel. Bounded by `timeout` — the question is *whether it
# starts writing into the operator's ~/.rustup*, not how long a 300 MB download
# takes. Killed early is a valid answer to that question.
printf '[toolchain]\nchannel = "1.74.0"\n' > "$WT/rust-toolchain.toml"
step "6b-toolchain-file-uninstalled-channel" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$FCARGO" \
  "cd '$WT' && (cargo build & p=\$!; sleep 25; kill -9 \$p 2>/dev/null; wait \$p 2>/dev/null); echo '(bounded at 25s)'"
rm -f "$WT/rust-toolchain.toml"

# =============================================================================
# ARM 7 — where does `cargo install` land? Proves the security claim instead of
# reasoning it. `--path` so it is seconds, not minutes.
# =============================================================================
step "7a-cargo-install-under-option-b" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$FCARGO" \
  "cd '$WT' && cargo install --path . --force 2>&1 | tail -5; ls -la '$FCARGO/bin' | sed 's/^/BIN /'"

step "7b-cargo-install-under-option-a" "$WHOME" "$WPATH_OPERATOR" \
  "RUSTUP_HOME=$REAL_HOME/.rustup CARGO_HOME=$REAL_HOME/.cargo" \
  "cd '$WT' && cargo install --path . --force 2>&1 | tail -5; ls -la '$REAL_HOME/.cargo/bin/spikey' 2>&1"
echo
echo "  --- undoing 7b: removing spikey from the operator's ~/.cargo/bin"
env -u RUSTUP_HOME -u CARGO_HOME HOME="$REAL_HOME" PATH="$WPATH_OPERATOR" \
    CARGO_HOME="$REAL_HOME/.cargo" RUSTUP_HOME="$REAL_HOME/.rustup" \
    sh -c "cargo uninstall spikey" 2>&1 | sed 's/^/      /'
echo "      still present? $( [ -e "$REAL_HOME/.cargo/bin/spikey" ] && echo YES-CLEAN-UP-BY-HAND || echo no )"

# =============================================================================
# ARM 8 — two workers, one shared CARGO_HOME, concurrent builds.
# =============================================================================
echo
echo "=== 8 · two-workers-one-cargo-home"
rm -rf "$FCARGO/registry" "$FCARGO/.package-cache" 2>/dev/null
ref="$WORK/.ref"; sleep 1; touch "$ref"; sleep 1
run_worker() {
  local n="$1" wt="$2" home="$3"
  env -u RUSTUP_HOME -u CARGO_HOME HOME="$home" PATH="$WPATH_PROXY" \
      RUSTUP_HOME="$REAL_HOME/.rustup" CARGO_HOME="$FCARGO" \
      sh -c "cd '$wt' && cargo build" > "$WORK/8-worker-$n.log" 2>&1
  echo "    worker-$n exit $?"
}
( cd "$WT"  && env -u CARGO_HOME sh -c "true" )   # no-op, keeps shellcheck honest
run_worker 1 "$WT" "$WHOME" &
run_worker 2 "$WT2" "$WHOME2" &
wait
for n in 1 2; do
  echo "    --- worker-$n tail"; tail -3 "$WORK/8-worker-$n.log" | sed 's/^/      /'
done
scan "$ref"

# =============================================================================
# ARM 9 (optional) — a fleet-owned RUSTUP_HOME, toolchains symlinked in. Can the
# last write path into the operator's HOME close for free?
# =============================================================================
echo
echo "=== 9 · fleet-owned-rustup-home"
rm -rf "$FRUSTUP"
mkdir -p "$FRUSTUP/toolchains"
cp "$REAL_HOME/.rustup/settings.toml" "$FRUSTUP/settings.toml" 2>/dev/null \
  && echo "    copied settings.toml ($(wc -c < "$FRUSTUP/settings.toml" | tr -d ' ') bytes)"
for tc in "$REAL_HOME"/.rustup/toolchains/*; do
  [ -d "$tc" ] || continue
  ln -sfn "$tc" "$FRUSTUP/toolchains/$(basename "$tc")"
done
echo "    toolchains symlinked: $(ls "$FRUSTUP/toolchains" | tr '\n' ' ')"
echo "    mirrored RUSTUP_HOME size: $(du -sh "$FRUSTUP" 2>/dev/null | cut -f1)"
step "9-fleet-owned-rustup-home" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$FRUSTUP CARGO_HOME=$FCARGO" \
  "cd '$WT' && cargo clean -q; cargo build && cargo --version"

# =============================================================================
# ARM 9b — the arm that decides between a real RUSTUP_HOME and a mirrored one.
# Same uninstalled channel as 6b, but pointed at the FLEET's rustup home. If the
# 35,981 files land here instead of the operator's ~/.rustup, Tier 1.1 holds and
# the last write path out of ~/.fleetor closes.
# =============================================================================
echo
echo "=== 9b · uninstalled-channel-under-fleet-owned-rustup-home"
echo "    uninstalling 1.74.0 from the operator's ~/.rustup first, so any hit there is fresh:"
env HOME="$REAL_HOME" PATH="$WPATH_OPERATOR" sh -c "rustup toolchain uninstall 1.74.0" 2>&1 | sed 's/^/      /'
rm -f "$FRUSTUP/toolchains/1.74.0-aarch64-apple-darwin"
printf '[toolchain]\nchannel = "1.74.0"\n' > "$WT/rust-toolchain.toml"
step "9b-uninstalled-channel-fleet-rustup" "$WHOME" "$WPATH_PROXY" \
  "RUSTUP_HOME=$FRUSTUP CARGO_HOME=$FCARGO" \
  "cd '$WT' && (cargo build & p=\$!; sleep 25; kill -9 \$p 2>/dev/null; wait \$p 2>/dev/null); echo '(bounded at 25s)'"
rm -f "$WT/rust-toolchain.toml"

# =============================================================================
# CLEANUP — undo everything these arms wrote outside work/.
# =============================================================================
echo
echo "=== cleanup"
env HOME="$REAL_HOME" PATH="$WPATH_OPERATOR" sh -c "rustup toolchain uninstall 1.74.0" 2>&1 | sed 's/^/    /'
[ -e "$REAL_HOME/.cargo/bin/spikey" ] && env HOME="$REAL_HOME" PATH="$WPATH_OPERATOR" \
  CARGO_HOME="$REAL_HOME/.cargo" sh -c "cargo uninstall spikey" 2>&1 | sed 's/^/    /'
echo "    toolchains left in the operator's ~/.rustup:"
env HOME="$REAL_HOME" PATH="$WPATH_OPERATOR" sh -c "rustup toolchain list" 2>&1 | sed 's/^/      /'
printf '    %-34s %s\n' "the operator's ~/.rustup" "$(du -sh "$REAL_HOME/.rustup" 2>/dev/null | cut -f1)"
printf '    %-34s %s\n' "stray spikey binary" "$( [ -e "$REAL_HOME/.cargo/bin/spikey" ] && echo 'PRESENT — REMOVE BY HAND' || echo gone )"
echo "    work/ still holds ~1.2 GB of downloaded toolchain; \`rm -rf examples/fence-toolchain-spike/work\`"

# =============================================================================
# ARM 10 — footprint.
# =============================================================================
echo
echo "=== 10 · footprint"
printf '    %-40s %s\n' "the operator's ~/.rustup" "$(du -sh "$REAL_HOME/.rustup" 2>/dev/null | cut -f1)"
printf '    %-40s %s\n' "the operator's ~/.cargo"  "$(du -sh "$REAL_HOME/.cargo" 2>/dev/null | cut -f1)"
printf '    %-40s %s\n' "the fleet's CARGO_HOME (after 8 builds)" "$(du -sh "$FCARGO" 2>/dev/null | cut -f1)"
printf '    %-40s %s\n' "the fleet's mirrored RUSTUP_HOME" "$(du -sh "$FRUSTUP" 2>/dev/null | cut -f1)"
echo "    what is in the fleet's cargo home:"
du -sh "$FCARGO"/* 2>/dev/null | sed "s|$WORK|\$WORK|" | sed 's/^/      /'

echo
echo "############################################################"
echo "# done. logs in examples/fence-toolchain-spike/work/"
echo "############################################################"
