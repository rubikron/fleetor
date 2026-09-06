# Developing FLEETOR

The dev loop, the zero-token run, the test tiers and the probes. **If you just want to
run the app, the README's *Run it* section is the whole story** — this file is for
changing it.

Read [`building.md`](../building.md) first for the decision tiers and the invariants;
nothing here overrides §1.

---

## 1. The dev loop

```bash
npm install
npm run tauri dev
```

`beforeDevCommand` builds the `fleet` CLI first (`cargo build -p fleetor-cli --bin
fleet`), because a pane with no `fleet` on its PATH looks alive and cannot talk. Vite
serves the frontend with hot reload; a Rust change restarts the shell.

**Two target directories, and it catches people.** `src-tauri/` is *not* a workspace
member, so it has its own `target/`:

| Binary | Built by | Lands in |
|---|---|---|
| `fleet` (the CLI panes run) | the root workspace | `target/{debug,release}/fleet` |
| the desktop shell | `src-tauri` | `src-tauri/target/{debug,release}/` |

`spawn::fleet_bin_path` knows this and checks four places in order:
`FLEETOR_FLEET_BIN`, then next to the running executable, then `../target/<profile>/fleet`
and `./target/<profile>/fleet` relative to the current directory. That last pair is why a
release binary run from the repo root finds its CLI and the same binary run from elsewhere
does not — see the README's *Run it* for the supported invocation, and set
`FLEETOR_FLEET_BIN` if you need another.

## 2. A run that costs nothing

```bash
FLEETOR_PANE_CMD=$PWD/tests/fake-pane/fake-pane.sh npm run tauri dev
```

Every pane runs a five-line echo script instead of `claude`. The registry, socket, hub,
delivery path and UI are all real; only the agents are not. **Use this for anything that
is not specifically about agent behaviour** — it is faster than the real thing and spends
nothing.

Drive it from another terminal:

```bash
export FLEET_SOCKET=~/.fleetor/_shell/fleet.sock FLEETOR_PANE=orch
./target/debug/fleet send 2 "hello worker two"
./target/debug/fleet roster
```

## 3. Tests

```bash
cargo test                      # the five crates
cargo test --manifest-path src-tauri/Cargo.toml   # the shell, incl. real-pty tests
npx tsc --noEmit -p tsconfig.json                 # the frontend
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

The shell suite drives **real ptys** in a dozen places and is the slow one; a full run is
a couple of minutes.

**Three tiers, and knowing which you are writing matters:**

| Tier | What it proves | Example |
|---|---|---|
| **Source-reading** | which code may *spell* a thing | `tests/placement_reads_nothing.rs`, `tests/gate_pickers.rs` |
| **Behavioural** | the code actually runs and produces the value | `tests/plan_seat.rs`, `tests/panes.rs` |
| **Render** | a component genuinely mounts and emits markup | `tests/gate_seat_row_renders.rs` |

C63 is the rule behind the third: a source-reading tripwire cannot tell you the component
it is quoting is the one that mounts, and this repo has shipped a screen that could not
load behind a fully passing tripwire. **When a change touches markup, add a render test
and prove it fails on a mutation** — the existing ones show the shape.

The render tier bundles `tests/gate_probe/render.tsx` with the `esbuild` already in
`node_modules` and runs it under `node`. No vitest, no jest, no new dependency (C24). It
**skips loudly** if `esbuild` or `node` is missing and **fails** if the probe or the
component is missing, because that is a deletion rather than an absent toolchain.

## 4. The vendor tier

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test vendor_binary_tier
```

Runs the real vendor binaries. It admits one probe run at a time and announces loudly
when it skips, so a machine without the tools reports that it measured nothing rather
than passing quietly.

## 5. Zero-token probes

`examples/plan-worker-spike/` holds the measurements behind the plan-backed worker
(`docs/notes/plan-worker-notes.md`). Every arm spawns an interactive vendor binary in a
pty, watches the screen for twelve seconds, and SIGKILLs — **nothing is ever submitted**,
so no inference is billed.

```bash
python3 examples/plan-worker-spike/probe.py --bin "$HOME/.local/bin/claude"
python3 examples/plan-worker-spike/probe_env_token.py --arm expiry
python3 examples/plan-worker-spike/probe_config_dir_creds.py --bin "$HOME/.local/bin/claude"
```

Point `--bin` at the real binary rather than the `claude` on your PATH: a wrapper shim in
front of it is not what a FLEETOR pane resolves (C47).

**Every probe redacts.** Credential material passes through one `redact` function that
every print path goes through, and the pty transcripts are filtered before they land.
Keep that property in anything you add here.

## 6. Conventions worth knowing before your first change

- **`decisions.md` is append-only.** A change that reverses a recorded decision annotates
  the old entry in place and appends a new one; it never edits history. Navigate with
  `grep "^## D-"` and `grep "^- \*\*C"`.
- **`docs/README.md` is the index, and a doc that is not listed there does not exist.**
  Updating it is the last act of any documentation change.
- **Measurement notes are never rewritten.** Where a note and a plan disagree, the note
  wins (`building.md` §4).
- **A test that encodes retired behaviour gets reshaped, not deleted.** The property
  underneath usually survives the change that broke the assertion; find it and pin that.

## 7. Releasing

There is no packaged build yet — `bundle.active` is `false` in `src-tauri/tauri.conf.json`
and no `externalBin` ships the `fleet` CLI beside the shell, so a `.app` would launch
without the binary its panes need. The README's *Run it* describes the supported way to
hand this to someone today. Wiring a real bundle means turning the bundle on **and**
shipping `fleet` next to the executable, which `fleet_bin_path`'s first candidate already
expects.
