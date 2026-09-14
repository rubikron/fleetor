# WP-29 — codex reaches History parity

status: not-started size: M + live spend
depends-on: 27 (S1–S3 landed), 28 blocks: —
brief-cost: 0 — nothing here touches `prompts/`

## Outcome

A mixed fleet reopens. **No new abstraction and no second History** — the `Harness` seam
already holds both vendors and the whole reopen path already calls through it. What is
missing is narrower and less visible: codex's implementations of the seam's methods are
*thinner* than Claude Code's, and the tests that would have shown it drive Claude Code
only. This package fills them in and tests them, closing WP-27's S4 and WP-28's codex half.

That distinction is the package's main risk. A seam gives every vendor a slot; it cannot
tell you a slot was filled with less than the other vendor put in it. Gap 1 below is
exactly that, and it survived the whole of WP-27 with a green conformance suite.

This is the second half of D-087. That decision ordered the *product* work — Claude Code
first, codex after — and this is "after". It does not reopen any R-number.

## Performance criteria

### Technical

- [x] **Landed (D-089).** `CodexCli::resume_args` carries the seat's posture forward the
      way `ClaudeCode`'s does: `--model` when `Seat::model` is set, and
      `--dangerously-bypass-hook-trust` on every non-operator seat. Pinned by
      `checkpoint_15_a_reopened_pane_keeps_every_posture_argument_a_fresh_one_gets`
      (`tests/harness_conformance_8_14.rs`), which compares `resume_args` against
      `command_args` per registered harness and is mutation-checked. Both flags verified
      accepted after the `resume` subcommand from `codex resume --help` at 0.153.4.
- [ ] A codex equivalent of `tests/placement.rs:1418`
      (`a_reopened_seat_is_placed_with_its_own_recorded_session`) — the spawn path asked
      codex, not just that codex can answer. R18's lesson, stated in WP-27: checkpoint 15
      answering proves a harness *can* resume; only a placement test proves the spawn path
      *asks*.
- [ ] `runs.rs`'s `a_mixed_runs_manifest_names_each_panes_harness…` asserts the codex
      seat's `session_id`, not only its harness name.
- [ ] The C39 snapshot allowlist is measured sufficient: a zero-token instrument opens a
      seeded pane's thread store and proves `sqlite_home` and `log_dir` actually relocate
      it. Notes committed to `docs/notes/` with a `docs/README.md` index row.
- [ ] The `tui.resume_cwd=current` override is re-verified **under R23's per-session
      worktrees**, both ways (launch cwd == recorded, and !=), against the current codex
      version. The version is stamped in the notes.
- [ ] Codex trust survives a reopen: a reopened codex worker meets no trust gate. Covered
      for both candidates — canonical cwd and the resolved git root, which for a linked
      worktree is the **main repository**.
- [ ] `tests/vendor_binary_tier.rs`'s codex arms are green, or each red arm has a
      `decisions.md` entry naming the cause. No arm stays red without a written reason.
- [ ] Quit-time archive verified live on a codex seat: `VACUUM INTO` leaves no `-wal`/`-shm`
      and the store stays resumable.

### Semantic

- [ ] A reader cannot tell from the reopen code which vendor was built first.
- [ ] Every claim in this package's notes says whether it was measured or read. The
      reopen spike's Claude Code half is the cautionary case: it proved the session
      plumbing appends while every turn returned `Not logged in`.

## Invariant guardrails

Tier 1.4 (nothing polls a live pane): session ids are read **at rotation**, after the run
is over — `runs::capture_session_ids`. Codex's reader must stay on that path.

Tier 1.2 (harness territory): the `Harness` seam stays harness-shaped. D-087 is explicit
that it orders the product work, not the seam — do not add a `match` on vendor name.
`placement/mod.rs:553`'s `codex::diagnose` call is the one existing exception and is not a
precedent to extend.

R13 (a resumed pane is told nothing) is what makes every gate fatal rather than annoying.
A trust gate or a cwd picker in a reopened codex pane hangs forever, because nothing will
answer it. This is why the trust and cwd criteria above are correctness items, not polish.

## Current state (verified 2026-09-11 — do not re-explore)

The seam and what codex already answers:

- `Harness` trait `src-tauri/src/placement/harness.rs:1233`; `resume_args` :1365,
  `session_id` :1375, `has_session` :1390, `command_args` :1351.
- Registry `static REGISTERED` harness.rs:1909, `registered()` :1913, `by_name()` :1927.
- `Resume::{Supported, NotSupported}` harness.rs:198-206. **No registered harness uses
  `NotSupported`** — codex declares `Supported` (codex.rs:857).
- Codex `resume_args` codex.rs:891 → `["resume", <uuid>, "-c", "tui.resume_cwd=current"]`,
  rationale at :877-890.
- Codex `session_id` codex.rs:908 — `thread_history_<n>.sqlite`, matched by name prefix,
  opened read-only: `SELECT thread_id FROM thread_items ORDER BY created_at_ms DESC LIMIT 1`.
  `has_session` codex.rs:927 — `SELECT 1 … WHERE thread_id = ?1`, `false` on any read error.
- Codex `command_args` codex.rs:1080-1092 — adds `--model` from the seat and
  `BYPASS_HOOK_TRUST` (codex.rs:1357) on non-operator seats only.
- Transcript archive: `Transport::SqliteBackup` (`VACUUM INTO`), store left in place so
  `codex resume` still finds it; pinned `src-tauri/src/runs.rs:1834-1907`.
- The only read of the operator's `~/.codex` is `read_operator_login` codex.rs:955 —
  `auth.json`. Nothing reads `~/.codex/sessions/**`; the rollout read is pane-local.

The reopen path codex has to travel:

- Resume argv selection `spawn::args_for` `src-tauri/src/placement/spawn.rs:191-201` —
  `ctx.resume` by pane name, else `command_args`.
- Gate `runs::reopen_blocker` runs.rs:1049, public wrapper `reopen_blocker_for` runs.rs:300;
  calls `has_session` at runs.rs:1091.
- Order held by a closure, not a line sequence: `runs::begin_reopen` runs.rs:312.
- Ids captured at rotation: `capture_session_ids` runs.rs:590.
- Quit archive: `fleet::quit` → `archive_after_teardown` → `archive_previous_run_under` →
  `runs::rotate` (D-086).

The gaps, all verified this session:

1. ~~**`CodexCli::resume_args` takes `_seat` and ignores it**~~ — **fixed, D-089.** It
   carried neither the model nor the hook-trust bypass, and the write guardrail for a codex
   pane runs *through* that hook table (codex.rs:1378-1384). Whether the omission was
   deliberate stayed unverified; no comment said so.
2. ~~**`checkpoint_15_…` never compares `resume_args` against `command_args`**~~ — **fixed,
   D-089**, which is what made (1) visible. The containment property is the reusable part:
   a third harness registered tomorrow cannot resume with a thinner argv than it launches
   with.
3. **No codex reopen test above the trait.** `tests/placement.rs:1418` drives
   `claude_code()` on `orch` only; every `runs.rs` lineage test uses
   `claude_code().spec()`. Codex has one unit test, `codex.rs:3125`.
4. **C39 allowlist sufficiency is unmeasured** — WP-27 open question 7. Codex's
   `seed_keys` are `sqlite_home` and `log_dir`, and that those settings relocate the
   stores is stated, not measured.
5. **Archived codex runs carry no per-turn usage** — `context_gauge.rs:316-322`. Known
   consequence of harvesting the thread store and not the rollout JSONL.
6. **Two red vendor-tier arms** — `typing-tuned-submits` (twice at `17050e7`),
   `brief-replaces` (once). WP-27 open question 9.

Prior measurements to read, not repeat: `docs/notes/reopen-spike-notes.md` (neither vendor
forks on resume; the cwd picker is cwd-*dependent*, correcting the parked WP-26 claim),
`docs/notes/codex-trust-key-notes.md` (two-candidate exact lookup, no ancestor walk, no
flag and no `-c` opens it), `docs/notes/codex-usage-notes.md`.

## Scope

**In:** the six gaps above. WP-28's codex half, which is only the live verification that a
codex seat's store archives correctly at quit — the archive code itself is vendor-generic
and landed.

**Out:**

- Per-turn usage for archived codex runs (gap 5). It is a second data source on the codex
  path and its own package; History does not need it.
- R11's harness-authoring guide. S4 bundled it with codex parity; it is not a codex
  problem and bundling it hid how much codex parity actually is. File it separately.
- R12's backend enforcement, still open from S2 and still UI-only. Not vendor-specific.
- Reading `~/.codex/sessions/**` or rollout JSONL for resume. The thread store alone drove
  resume in the ticket-06 shakedown; rollout JSONL is legacy on this version.
- Per-session SQL extraction from the thread store. R4's per-run directory exists
  precisely so this is never needed, and C12 records codex generation-numbering the store
  because it expects the schema to move.

## Design sketch & open questions

1. **Does `resume_args` carry posture, or does `args_for` merge it?** **Recommended:**
   carry it in `resume_args`, matching Claude Code, and pin the agreement in conformance.
   A merge in `args_for` would put vendor knowledge on the generic path.
2. **Is losing `--dangerously-bypass-hook-trust` on reopen a safety regression or a safety
   improvement?** It is not obvious: the bypass exists because FLEETOR owns the hook table
   (codex.rs:1388), and without it a reopened pane's guardrail plausibly does not run —
   but "plausibly" is a read, not a measurement. **Recommended:** measure what a reopened
   codex pane does to a write it should refuse, before deciding. If the guardrail still
   fires, the flag may be droppable on resume *deliberately*, with a comment.
3. **How is the C39 measurement instrumented at zero token spend?** **Recommended:** seed
   a `CODEX_HOME`, launch codex far enough to write a thread, kill it, then open the store
   where the seed says it should be. No assistant turn needed — the plumbing writes before
   the model answers, which is exactly what the reopen spike's `Not logged in` half proved.
4. **Do the red vendor-tier arms predate S2?** Unchecked. **Recommended:** check out
   `3e2937e` and run the probe before diagnosing anything — `examples/codex-spike/probe.py`
   reads no repo code, so a pre-existing failure is a probe/version problem, not ours.
5. **What does a pre-R23 codex run reopen into?** R23 left this open for Claude Code too.
   **Recommended:** treat it as one question for both vendors, answered here because this
   package is where a mixed reopen first runs.

## Session prompt

```
Read docs/roadmap/29-codex-history-parity.md in full, then building.md §1 and §9, then
docs/roadmap/27-reopening-a-run.md's Slices and open questions, then the "History redesign
— resume past runs" section of decisions.md (R0–R24) and D-087.

Then read, and do not re-measure: docs/notes/reopen-spike-notes.md,
docs/notes/codex-trust-key-notes.md. The cwd picker is cwd-DEPENDENT; the parked note
under .scratch/ says otherwise and is wrong.

Do not re-explore for anything in the doc's "Current state" section; those anchors were
verified 2026-09-11. Re-verify them mechanically before you commit, and say so if any
moved.

This work needs live spend: a mixed codex + Claude Code fleet, reopened. Name the spend
and get the operator's approval before spending it (building.md §9.5). Everything in the
C39 measurement is designed to cost zero tokens — do that part first.

Work the gaps in the order they are numbered. Gap 1 is a defect with a test-shaped fix and
should land first, alone, so its commit is reviewable.
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite the D-number in the
      commit subject), and one for gap 2's measurement whichever way it lands.
- [ ] Spike notes committed to `docs/notes/`, version-stamped, with a `docs/README.md`
      index row.
- [ ] WP-27's S4 marked closed, its open question 7 and 9 answered or re-parked with a
      reason.
- [ ] `28-archive-on-quit.md`'s codex line resolved.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column and dependency edge updated — the last act of the session.
