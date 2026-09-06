# WP-25 — the `Harness` seam, and codex as the first harness through it

status: landed size: L
depends-on: 20 (placement, landed) blocks: 24 (cursor, re-scoped onto this seam)
brief-cost: 0 — the briefs do not change. One `orch.md`, one `worker.md`, every harness (M4).

**The spec lives in [issue #13](https://github.com/rubikron/fleetor/issues/13), not here.** This doc
is the roadmap's pointer to it: what it is, what was decided, what was measured, and the
anchors an implementing session needs. The issue carries the problem statement, 58 user
stories, the implementation and testing decisions, the design trail and the out-of-scope
list. The decision trail — every option considered, every rejected alternative, and what
would reverse each choice — is `decisions.md` **C1–C24**. WP-24's **M1–M28** is the prior
arc's trail and is still authoritative for everything C-series does not amend by name.

## Outcome

The harness stops being an assumption and becomes a choice the operator makes at the start
gate, beside the target repository. A fleet can be all Claude Code, all codex, or mixed,
and the panes collaborate through the same nine verbs regardless. Underneath, the thing
that made this hard becomes the thing that makes the next harness easy: a `Harness` seam
with fourteen named checkpoints and a conformance suite, so adding a third TUI is a
checklist rather than an archaeology project.

**This arc owns the seam. WP-24 shrinks to registering cursor through it (C19).**

## Why codex goes first (C18)

The land order reversed after measurement. On every one of the fourteen checkpoints codex
is strictly cheaper than cursor:

| | cursor (WP-24) | codex (this arc) |
|---|---|---|
| brief carrier | `.mdc` **in the worktree** + `.git/info/exclude` (M5) | `model_instructions_file`, **replaces** the prompt, no repo file (C3) |
| write guardrail | no pre-edit event; the sandbox stands in (M12) | a real `pre_tool_use` hook — D-065 ports (C4) |
| worker credential | holds the operator's, which a CC worker is denied (M8) | holds the fleet's, same as a CC worker (C9) |
| orphan sweep | a Node tree, without matching a bare interpreter (M23) | a native binary — one word (C11) |
| transcript | format unknown, spike-gated (M22) | versioned SQLite, schema read (C12) |
| tests | artifact assertions or tokens | **vendor binary, zero tokens** (C13) |

The seam's claim is that adding a harness is a checklist. That claim is tested honestly by
the harness whose fourteen answers are already green — not by the one whose unknowns
(does an `alwaysApply` `.mdc` survive `/clear`, what is cursor's transcript format) are
still open.

## What was measured, not assumed

Design ran spike-first (`building.md` §4) against the real `codex` (`codex-cli 0.153.4`).
Each finding is reproduced in the issue; the reversals are its **Design Trail** section.

- **`model_instructions_file` replaces the system prompt** — read off the wire, not
  inferred. A fabricated `[model_providers.probe]` pointed at a local capture server
  rendered the literal request body: `instructions` went from the built-in **17,730** chars
  to the **55**-char sentinel, and "You are Codex" was gone. D-043's property exactly.
- **Two carriers measured and rejected.** `base_instructions` in a custom
  `model_catalog_json` is **not honoured**; `AGENTS.md` arrives as a **`user` message** —
  in-band, spending the pane's own context.
- **`CODEX_HOME` carries config *and* login.** A fabricated `HOME` with the real
  `CODEX_HOME` stayed logged in; an empty `CODEX_HOME` reported "Not logged in". **M8's
  auth-follows-`HOME` fact is cursor's alone** — this is what renamed checkpoint 6.
- **The socket refusal and its one lever.** Under `sandbox_mode = "workspace-write"` a unix
  socket `connect()` is refused (`Operation not permitted`); with
  `sandbox_workspace_write.network_access = true` the identical probe printed `CONNECTED`
  and the listener got its bytes. **No bridge**, exactly as M28 concluded for cursor.
- **The narrow lever does not work.** `network.unix_sockets` and
  `network.dangerously_allow_all_unix_sockets` read like a targeted allowance and would
  have beaten M14's all-or-nothing. Three variations, including `--enable network_proxy`,
  left the refusal in place: they configure the network-*proxy* layer, not the seatbelt.
- **The fence is real.** `echo x > $HOME/probe` under the sandbox: refused, no file.
- **47 feature flags are on by default**, four of which reach past that fence:
  `multi_agent`, `browser_use` (with `browser_use_full_cdp_access`), `computer_use`,
  `in_app_local_automation` (C21).
- **Delivery works, and needed tuning.** A bracketed paste + CR into a real pty submitted
  and produced a request carrying a sentinel absent from the pasted bytes — **on the second
  attempt**. 6 s startup / 0.6 s post-paste: nothing. 12 s / 1.5 s: submitted. **Codex is
  the first harness that owes M6 a real typing profile** (C22).
- **`codex doctor --json` in 1.36 s** returns `schemaVersion`, `codexVersion`, auth mode,
  provider, model and the resolved sandbox posture — checkpoint 14 and C8's tripwire in one
  call. Caveat: it proves *reachability*, not authorization (HTTP 401 passed).

## Current state (verified 2026-09-05 — do not re-explore)

- `src-tauri/src/placement/mod.rs:455` — `place`, **the one seam**. All fourteen checkpoints
  are observable through it; `Layout` and `Host` already arrive as values.
- `src-tauri/src/placement/mod.rs:285` — `Host`, where harness discovery belongs (login,
  account shape, model list, resolved posture) so `place` keeps its "nothing reads the
  process" rule. `Host::discover` (`:322`) already shells out per spawn.
- `src-tauri/src/placement/spawn.rs:116` `orch_command_with`; `:286` `worker_command_with`;
  `:367` `base_command_with` (the literal `claude`); `:543` `seed_config_dir`.
- `src-tauri/src/placement/spawn.rs:331` — **`ANTHROPIC_BASE_URL` = `https://api.deepseek.com/anthropic`,
  `ANTHROPIC_AUTH_TOKEN` = `.env`'s key, and `:344` *unsets* `ANTHROPIC_API_KEY`.** This is
  C9's whole argument: a CC worker already runs on the fleet's credential, so a codex worker
  can too, and M8's exception is dissolved rather than imported.
- `src-tauri/src/placement/spawn.rs:543` — `seed_config_dir` already writes
  `hasTrustDialogAccepted` per project. C16's codex form is `[projects."<wt>"] trust_level`.
- `src-tauri/src/placement/spawn.rs:583` — `project_key`, Claude Code's **own**
  canonicalization (`std::fs::canonicalize`), shared by the trust flag and the gauge.
- `src-tauri/src/guardrail.rs` — the `PreToolUse` half, 18 references. Ports to codex (C4).
- `src-tauri/src/orphans.rs:51` — `sweep_at_named(path, "claude")`; `:77` `reap_if_named`
  confirms the comm suffix against pids **FLEETOR recorded**. One more name (C11).
- `src-tauri/src/runs.rs:226` — `harvest_transcripts`, Claude-Code-only; `runs.rs:64`
  documents `0` as ordinary — a real gap wearing a normal run's clothes.
- `src-tauri/src/context_gauge.rs:50` — `WORKER_WINDOW_TOKENS` (D-054). Codex publishes
  `context_window` per model instead.
- `src-tauri/tests/placement.rs` — the prior art. Scratch layouts, values not env vars,
  caller-observable assertions only. The conformance suite is one pass per harness here.
- `ui/src/components/StartGate.tsx` — the spend gate the pickers join (M15, C23).

## Scope

**In:** the `Harness` trait + `HarnessSpec` inside `placement` with the conformance suite
(M23); the fourteen checkpoints with 6 and 14 renamed (C17); the zero-token vendor test
tier (C13); codex registered — `CODEX_HOME` snapshot, brief, sandbox trio, FLEETOR
provider, worktree trust, feature overrides (C3, C6, C7, C9, C16, C21); the gate — probe,
pickers, cost line, posture tripwire (C8, C14, C23); the integration tail — gauge, archive,
orphan sweep, project-key split (C11, C12).

**Out:** mid-run harness switching; harness choice for the evaluator and the Critic; a
per-seat *provider* picker (C2 as amended by C9); per-pane budgets; quota-exhaustion
detection; transcript normalization; reconstructing gauge usage if codex does not persist
it; migrating to the `[permissions]` generation; making codex subagents / browser / computer
use work inside the fleet; **cursor** (that is WP-24, now downstream); a frontend test
runner. **Also out, deliberately:** the CLI's identity and authorization model — M27's
deferral stands, unconditional, because nothing here re-executes fleet commands out of band.

## Invariant guardrails

- **Tier 1.4 — nothing on the message path.** The existing hub and CLI suites passing
  **unchanged** is the evidence, the way `write_guardrail.rs` asserts the same property for
  the guardrail.
- **Tier 1.7 — auto-approve is narrowed, not widened.** A codex worker's sandbox refuses a
  superset of what the write guardrail refuses (C7), and C21 narrows further still.
- **D-030/D-052 are amended, not ignored.** "The orchestrator is the operator's own
  `claude`" stops being an invariant once that seat can be another vendor; a
  `default (your login)` sentinel keeps today's behaviour reachable (M2).
- **D-042 holds.** One `orch.md`, one `worker.md`, every harness. Only the carrier varies.
- **D-062 holds, and now for three harnesses.** A worker holds the fleet's credential, not
  the operator's (C9) — where M8 had to record an exception, this arc has none.
- **M25 — the roster stays harness-free.** Workers see each other as equals; the operator's
  rail carries the fact instead.
- **M10 gains its first inverse (C21).** Overlays are additive and `deny` wins — except the
  two sandbox keys FLEETOR owns, and the four features it turns off on worker seats.

## Phases (C20)

1. The `Harness` trait + `HarnessSpec` inside `placement`, the conformance suite over the
   fourteen checkpoints as renamed, **Claude Code the sole registered harness, green** —
   and C13's zero-token vendor tier, built here because it belongs to the suite rather than
   to codex.
2. Codex registered: `CODEX_HOME` snapshot seeding, the brief, the sandbox trio, the
   FLEETOR provider, per-worktree trust, the feature overrides. **Two spikes belong here:**
   whether the brief survives `/clear`, and how codex resolves the trust key.
3. The gate: the `doctor` probe, the pickers, the per-harness cost line, the posture
   tripwire.
4. The integration tail: gauge reader, transcript harvest (SQLite backup, **not** `cp`),
   orphan sweep names, project-key split. **The gauge's usage spike is decided here.**

## Session prompt

```
Read docs/roadmap/25-codex-tui.md in full, then the spec at
https://github.com/rubikron/fleetor/issues/13, then decisions.md C1–C24 (search "WP-25 —
codex TUI"; the C-series is the interview trail). Read M1–M28 too — the prior arc — and
note that C2, C17 and C19 amend it by name. Then building.md §1, §4 and §9.

Implement phase 1 only: the Harness trait and HarnessSpec inside placement, the
conformance suite over the fourteen checkpoints (checkpoint 6 is "config and credential
isolation mechanism", checkpoint 14 is "project identity and trust seeding" — C17), with
Claude Code as the sole registered harness, plus C13's zero-token vendor tier as suite
infrastructure. No codex code in this phase — the suite must be green with one harness
before a second exists, or it is not a suite, it is a description of codex.

The seam is placement::place (placement/mod.rs:455). Do not add a second entry point and
do not add a crate — M23's reasoning, unchanged.

Spike-first for anything the spec marks unverified (building.md §4). It marks two, both
phase 2: whether model_instructions_file survives /clear, and how codex resolves the
project trust key. Do not start them here.

Exit: the checklist at the bottom of this doc.
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite C-number and new
      D-number in the commit subject).
- [ ] No verb was added or changed — if that stops being true, both prompt files + `VERBS`
      + the clap enum + pinned tests move in one commit.
- [ ] Spike notes committed to `docs/notes/`, version-stamped with the **vendor build id**
      (`codex-cli 0.153.4`), with a `docs/README.md` index row. C13 makes the probes suite
      infrastructure, so they are committed as runnable, not pasted as transcript.
- [ ] The hub and CLI suites pass **unchanged** — that is the Tier 1.4 evidence, not a
      formality.
- [ ] The conformance suite is green with **one** registered harness before phase 2 opens.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.

---

## How it landed

**Landed across 43 commits on `codex-tui`, as 37 tickets (#14–#48).** The `Harness` seam holds two
harnesses; the conformance suite runs all fourteen checkpoints over both, green. `src-tauri` went
from 64 tests to 389. **The root workspace held at 226 through every single commit** — that number
is Tier 1.4's evidence, and it never moved.

The decision trail is `decisions.md` **C25–C68**, continuing C1–C24. Six entries carry in-place
annotations where a later measurement falsified an earlier conclusion — **C14, C16, C46, C47, C49**
— so a reader arriving at one is pointed forward instead of being misled.

### The phase order paid for itself, and the receipt is six reshapes

C20's rule was that the suite must be green with **one** registered harness before a second exists,
or it is a description of that second vendor wearing an abstraction's clothes. The registry held at
one from #14 through #39. When #33 finally registered codex, **six checkpoints turned out to be
shaped around Claude Code, four of them invisible until a second pass ran**: the key-list assertion
matched flat keys where codex's are nested; checkpoint 6 `panic!`d outright on a harness that
snapshots from an operator directory; checkpoint 14 parsed the trust file as JSON when the format is
the vendor's; checkpoint 14's negative forbade a record codex both writes *and reads*; checkpoint 13
required a non-empty `subdir` when the empty string is a real answer; and the driver's `echo_of`
waited from position zero, so for a harness that presses a key during bring-up (#42) checkpoints 9
and 10 would have asserted against the wake's reply rather than the write under test.

**So the arc's own claim — "adding a harness is a checklist rather than an archaeology project" — is
now testable rather than asserted, and its first honest reading is six reshapes and two undiagnosed
blockers** (#44, #39). Cheaper than archaeology, dearer than a checklist (C57).

### What the spikes reversed

Three recorded decisions were falsified by measurement rather than argued away.

- **C16's "codex resolves trust by root" was wrong** (C34). It resolves by a **two-candidate exact
  lookup** — canonicalized cwd *or* the git root — with no ancestor walk, and for a linked worktree
  that root is the **main repository**. The obvious computation (`--show-toplevel`) produces the key
  that does not cover subdirectories.
- **C46's shim-clobber did not exist** (C48). `-c` and `config.toml` hooks compose; what replaces is
  one `-c` by a *later* `-c`. And a `-c`-delivered hook does not fire untrusted either — trust is a
  property of the hook, not of its delivery.
- **C14's plan tier is not in `doctor`** (C58). It reports the auth *shape*; the tier lives inside
  the stored id token.

### Five defects the spec did not anticipate

Each was found by an agent building the thing, and each shipped as its own ticket rather than a
quiet fix.

| | Found by | What it was |
|---|---|---|
| #42 | #24, by accident | A fresh `CODEX_HOME` opens on a splash that ends on a **keypress, not a timer**, and swallows the first message — `accepted` for a message nobody read |
| #43 | #27 | The vendor tier **collided with itself**, reporting contention as a regression |
| #44 | #29 | A codex orchestrator had **no login at all** — checkpoint 6 was shaped around a vendor whose credential is a keychain pointer |
| #45/#46 | #31 | The write guardrail **installed correctly and never fired**; untrusted hooks are silently skipped |
| #47 | #30 | The bring-up wake was **dismissing the trust gate and persisting `trust_level = "trusted"` to disk** |

### Two costs stated rather than hidden

**The write guardrail fires only under `--dangerously-bypass-hook-trust`, and only because FLEETOR
owns a worker's `hooks` array outright** (C49, C50). A worker executes only hooks the fleet authored,
so trusting them is trusting our own artifact and the pane's executable surface *narrows*. The price
is real: an operator's own `PreToolUse` hook does not run on worker seats. The orchestrator inherits
untouched and gets no bypass.

**The vendor tier now costs 84–103 s** (was ~47 s), because `probe_clear.py` is finally gated
(C67). D-081 chose default-on at a stated cost; this is that cost moving, stated.

### One thing worth knowing before the next arc

**A source-reading tripwire proves the code *says* the right thing, never that it *runs*** (C63).
#35's gate pickers passed a 15-check tripwire with a working negative control while being
**unreachable** — `fleet_gate` required a running fleet and the gate renders before bootstrap. That
is the standing cost of C24's no-frontend-test-runner rule. Any ticket whose value is a *check* owes
a behavioural test that the check **fires**.

### Spend

**One turn, 12,548 tokens** (#38, C61) — the arc's only authorized spend, against the native vendor
binary. It answered *no*: codex does not persist per-turn usage in its thread store. It persists it
**twice beside it**, so the gauge reads the vendor's own accounting rather than `unavailable`.
