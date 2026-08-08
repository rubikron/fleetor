# WP-06 — Receipts, peer review, and the merge step

status: **landed** (2026-08-06, D-048..D-050) size: L
depends-on: 01, 05 blocks: —
brief-cost: +~150 tokens across both prompt files (done/receipt protocol, reviewer duty, merge rule)

## Outcome

The execution phase closes its loop. A worker finishing a task block runs `fleet done "<check command>"` — the CLI runs the command **locally in the worker's own worktree**, then sends orch an ordinary message containing the **receipt**: real exit code, current commit hash, and a capped output tail. Peers review each other's work **without leaving their own worktrees** (all worktrees share the target repo's git object DB — `git diff fleet/worker-N`, `git log fleet/worker-N` work today, zero code). Orch merges reviewed branches into **`fleet/integration`** — never trunk; the operator merges trunk manually. This implements "workers consult other workers … like a code review" and closes the named gap "nothing ever merges the workers' branches."

**The worktree reconsideration is answered (operator decision 2026-08-06): worktrees stay.** (a) Tier 1.7's auto-approve boundary *is* the worktree; (b) separate git indexes are why four workers can commit concurrently; (c) review needs no shared checkout — the shared object DB suffices. The implementing session validates (c) live and records the decision in `decisions.md`.

## Performance criteria

### Technical
- [ ] `fleet done` runs the check command **in the CLI process** (the worker's own cwd/worktree). The hub never executes anything.
- [ ] **The CLI's exit code reflects delivery, not the check.** The delivery contract ("non-zero exit == not delivered") is the hardest-pinned assertion in the codebase; the check's exit code travels *in the message body*. Do not conflate them.
- [ ] The receipt is an ordinary `fleet send` — no new event kind, no DB change (bodies are already logged). Output tail capped at the CLI (~2 KB recommended); `sanitize` already strips escapes.
- [ ] Review requests/replies are ordinary sends quoting the task id. Stretch (optional): `fleet reply` auto-tags the replied-to message id — the surviving half of the Callsign design.
- [ ] Merging is git-only, on `fleet/*` branches, executed by orch's own Bash in its cwd (the target repo). `rm -rf ~/.fleetor && git worktree prune` still leaves the repo clean — `fleet/*` branches are the "minus kept feature branches" carve-out Tier 1.1 already names.
- [ ] The shared-checkout fallback (`fleet.rs:284-296` — when the target isn't a git repo, workers share the target directory) gets a **louder warning**: review semantics collapse there (one checkout reported five times).
- [ ] Live validation: from worker-1's worktree, `git diff fleet/worker-2` shows worker-2's commits (shared object DB claim proven, recorded in the notes).

### Semantic
- Worker prompt: *`done` means the criteria commands actually ran — send the receipt, then ask the reviewer named in the task block; reviewers diff the branch and answer against the task's criteria, not taste.*
- Orch prompt: *merge to `fleet/integration` only after a review reply exists; record what was merged on the board (`fleet task update`).* A prompt rule — never a code gate.
- The vision link: this is the "workers make sure the situation fits the criteria, and consult other workers" clause, made mechanical enough to be honest (receipts) and social enough to stay out of the message path (review is conversation).

## Invariant guardrails

- **Tier 1.1:** trunk is the operator's. Merges land on `fleet/integration` only. FLEETOR writes only application code, only on feature branches.
- **Tier 1.7:** review access is read-only via the shared object DB from the reviewer's own worktree. Widening auto-approve or cd-ing workers into each other's checkouts is a security escalation (`building.md` §9.2) — above the builder.
- **Tier 1.8:** "shared knowledge merges only after review" — this package is that invariant finally getting its feature. Any culture/lessons files a session is tempted to add follow the same rule.
- **No hub-side verification of receipts, no review gates in code, no auto-merge.** The receipt is evidence; judgment stays with agents.

## Current state (verified 2026-08-06 — do not re-explore)

- Worktrees: `~/.fleetor/_shell/worktrees/<target-slug>/worker-N` on branches `fleet/worker-1..4` (`src-tauri/src/fleet.rs` — `worktree_dir` `:191`, `ensure_worktree` `:760`, `worker_cwd` fallback `:735`). Namespaced by target (dirname + 4-char path hash), so switching targets preserves existing worktrees and creates fresh ones for the new target. Created with `git worktree add -B` from the target; object DB is shared by construction.
- CLI: `crates/fleetor-cli/src/main.rs` — one connection, one op, exit; reads `FLEETOR_PANE`/`FLEET_SOCKET`; `report()` prints `accepted <msg_id>` / `fleet: not delivered — <detail>`. `fleet done` composes a body and reuses `Op::Send { to: orch }`.
- Delivery contract text: post-WP-01 in `prompts/delivery-contract.md`; the verb/prompt validation coupling applies — `done` must be taught in both prompt files, `VERBS`, clap, and pinned tests in one commit.
- Interaction with WP-08: a private HOME removes global `user.name`/`user.email` — worker commits fail. Whichever of 06/08 lands second re-validates worker commits (08 seeds a gitconfig).

## Scope

### In
`fleet done` verb (local check run, receipt compose, existing send path); prompt protocol text for done/review/merge; the worktree decision + shared-object-DB validation recorded in `decisions.md` and a short notes doc; louder shared-checkout fallback warning; optional reply auto-tag stretch.

### Out
Hub-side execution or verification; review gates in code; auto-merge; touching trunk; cross-worktree write access; per-task worktrees.

## Design sketch & open questions

1. **Receipt format.** Recommended: first line `[receipt] task-<id> · fleet/worker-2 @ <short-hash> · exit <code>`, then a fenced output tail. One glance answers "what, where, did it pass."
2. **Tail cap.** Recommended ~2 KB — enough for a test summary, not enough to flood a pane's context.
3. **Does `fleet done` also post `fleet task update done`?** Recommended: no — one verb, one job; the worker posts the status update as its own act (attribution stays honest).
4. **Reviewer assignment.** Recommended: a field on the task block (WP-05's schema has room); orch names the reviewer at post time.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/roadmap/06-receipts-review-merge.md in full, then
building.md §1 and §9. Execute WP-06: the fleet done verb (check runs
locally in the CLI, receipt travels as an ordinary message, CLI exit code
still means delivery), the review/merge protocol in both prompt files
(review from the reviewer's own worktree via the shared object DB; orch
merges reviewed branches to fleet/integration, never trunk), the louder
shared-checkout fallback warning, and the worktrees-stay decision
validated live and recorded in decisions.md. No gates, no auto-merge, no
hub-side execution. Prompts/VERBS/clap/tests move in one commit. Finish
with the session exit checklist.
```

## Session exit checklist

- [ ] Full test matrix green; delivery-contract tests untouched and green.
- [ ] Shared-object-DB review validated live and noted.
- [ ] `decisions.md`: worktrees-stay entry + the verb entry.
- [ ] Both prompt files + `VERBS` + clap + pinned tests in one commit.
- [ ] `00-index.md` status updated; if WP-08 already landed, worker commits re-validated.

## How it landed (2026-08-06)

Commits `959face` / `6f75066`, decisions D-048 (worktrees-stay, review via the shared object DB), D-049 (`fleet done` runs the check locally in the worker's worktree and sends the receipt as an ordinary message — CLI exit code still reflects delivery, not the check), D-050 (orch merges reviewed branches to `fleet/integration`, never trunk; the auto reply-tag stretch item not taken). Measurement in `docs/notes/peer-review-notes.md`; the verb's local half lives in `crates/fleetor-cli/src/done.rs`. As-built map: `docs/fleet-comms-map.md` §3d. The checkboxes above were not ticked by the landing session; the D-entries are the record.
