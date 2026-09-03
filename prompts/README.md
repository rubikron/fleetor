# `prompts/` — everything a pane is told

Every word injected into a pane's `claude`, and every flag it is launched with, is a file in this directory. Nothing is spelled out in Rust: `fleetor-core::brief` bakes these in with `include_str!` and fills their placeholders, and `src-tauri::prompts` lets you override them without a rebuild.

For *when* each piece arrives and where it lands inside the pane's context window, see [`docs/context-injection-flow.md`](../docs/context-injection-flow.md).

## The files

| File | What it is | Placeholders it must keep |
|---|---|---|
| `orch.md` | The orchestrator's brief | `{cwd}` `{workers}` `{delivery_contract}` `{scaffolding}` `{vision_tenets}` |
| `worker.md` | The brief every worker slot renders | `{me}` `{cwd}` `{peers}` `{delivery_contract}` `{broadcast_rule}` `{scaffolding}` |
| `critic.md` | The Critic's brief — reads one run's archive and reports what the fleet did | `{archive}` |
| `delivery-contract.md` | Fragment: what a `fleet` exit code means, and the three outcome words | — |
| `broadcast-rule.md` | Fragment: never answer a broadcast unless it names you | — |
| `scaffolding.md` | Fragment: the working posture CC's own prompt used to supply | — |
| `vision-tenets.md` | Fragment: how the orchestrator thinks about vision | — |
| `launch.conf` | Model, endpoint and flags a worker starts with | — |

All four workers render the same `worker.md`. They differ only in `{me}`, `{peers}` and `{cwd}`.

**These briefs are the pane's *whole* system prompt (D-043).** They go in via `--system-prompt`, which replaces Claude Code's own rather than appending to it. What that does and does not take away is measured in [`docs/notes/system-prompt-notes.md`](../docs/notes/system-prompt-notes.md) — the short version is that CC's guidance leaves and its tools, memory files, skills and git-status section stay. `scaffolding.md` is the part worth restating, and `{cwd}` covers the one thing genuinely lost.

## Placeholders

| Placeholder | Fills with |
|---|---|
| `{me}` | this pane's name — `worker-2` |
| `{peers}` | the roster minus this pane, in prose — `orch, worker-1, worker-3 and worker-4` |
| `{workers}` | the roster minus `orch` — `worker-1, worker-2, worker-3 and worker-4` |
| `{cwd}` | the directory this pane was started in — a worker's own git worktree, or the target itself for `orch` |
| `{delivery_contract}` | the whole of `delivery-contract.md` |
| `{broadcast_rule}` | the whole of `broadcast-rule.md` |
| `{scaffolding}` | the whole of `scaffolding.md` |
| `{vision_tenets}` | the whole of `vision-tenets.md` — orchestrator only |
| `{archive}` | the run directory the Critic was pointed at — Critic only |

Anything else in braces is left alone — these are markdown files, not format strings, so `Vec<{}>` in prose survives.

## Why four of them are fragments

`delivery-contract.md`, `broadcast-rule.md`, `scaffolding.md` and `vision-tenets.md` are composed *into* the briefs at a placeholder rather than written out in them, and a template that drops its placeholder is **refused** rather than rendered.

All four are load-bearing:

- The **delivery contract** is the only reason a model can tell a failed send from a good one. It reads its own Bash exit code and self-corrects. A pane without it silently believes every message arrived.
- The **broadcast rule** is the only mitigation left for broadcast amplification. The hub-side rate limiter was deliberately removed (`decisions.md` D-031) on the grounds that nothing in the delivery path may be able to refuse a message — which is only safe while this clause holds. Five peers that all answer every broadcast is a token fire that looks like a working fleet.
- The **vision tenets** are `docs/roadmap/source/vision_tenets.md` carried as the operator wrote it (D-056, reversing D-044's distillation at the operator's direction — the condensed version undermined it; only the source newsletter's PDF/podcast/YouTube plugs are dropped). Split out so you can change how the fleet *talks* about vision without editing what it *does* — the rules that bind (confirm in writing before decomposing, propose bigger once, announce deviations) live in `orch.md` and are pinned by tests.
- The **scaffolding** is everything a pane used to inherit from Claude Code's system prompt and no longer does (D-043): tool discipline, what a denied call means, that a long conversation is summarized rather than ended, faithful reporting of outcomes, and the refusal and pronoun defaults. Rewrite it freely — but a brief that drops it is a pane with a job description and no working posture.

So you can rewrite every word around them and they still arrive. You edit the prose; the fleet keeps its contracts.

## Overriding without a rebuild

Copy any of these files to `~/.fleetor/prompts/` and edit it there. The app reads that directory **once at bootstrap** and announces what it found on the Activity feed:

- **Loaded** → an `Info` notice naming the file. You always get told an override took effect, because a prompt change that quietly did nothing is indistinguishable from one that did not work.
- **Broken** → a `Warn` notice saying exactly what to fix, and the built-in is used. Never a half-applied brief.
- **Absent** → nothing. That is the ordinary case.

Changes take effect **the next time the fleet starts**, not on a running one — a system prompt cannot be changed after the process is exec'd, and it is the same rule the target picker follows.

```console
$ mkdir -p ~/.fleetor/prompts
$ cp prompts/worker.md ~/.fleetor/prompts/
$ $EDITOR ~/.fleetor/prompts/worker.md
```

To go back to the built-in, delete the file.

## `critic.md` is here because the Critic is a product feature (D-076)

The Critic is not a pane in the fleet — it appears in no roster, is no leg of a broadcast, and is in no brief's peer list — so `{me}` and `{peers}` mean nothing to it and it teaches no verbs. It is here anyway, and that is the point: it reads one run's archive and reports what the fleet did, which is a judgement the operator is meant to tune. Rewrite it as you like.

Two clauses are doing the work, and `docs/notes/critic-spike-notes.md` §8 says so with a measurement behind it: **every finding carries a citation**, and **nothing is prescribed**. The same archive read with an unconstrained prompt produced fluent, largely *true*, entirely uncitable prose that quietly graded the work — worse than the bland summary the roadmap predicted, because an operator cannot tell its true claims from its unfalsifiable ones. Drop either clause and that comes back. The spike's own advice is to re-run the control before shipping a trim.

The brief reports on **six** categories. Five are the roadmap's (WP-20 D10); the sixth — *"Anything else the fleet did that cost the operator"* — was added during the spike, where it produced the single largest finding on both measured runs while D10's third and fourth categories produced nothing on either. It is a bounded slot, not a free-form opinion slot: it carries the same citation rule, bans inference and prescription by name, and sends any moment you cannot cite to UNCITED. That wording is what kept it from reopening the door.

`{archive}` is the one thing an override may not drop. A Critic that does not know which directory to read spends its first turn asking, so a template missing it is refused with a `Warn` and the built-in is used — the same treatment `orch.md` and `worker.md` get for a missing fragment.

## The one brief with no override, and why

Every file in the table above can be replaced from `~/.fleetor/prompts/`. **There is exactly one brief in this application that cannot be, and it is not in this directory: the evaluator's** (WP-15). It is not a fleet brief — the evaluator is not a pane in the fleet — and it lives in a separate repo, compiled into the binary at build time behind the `devmode` cargo feature. This repo's source carries none of its prose, and in a dev build no file on disk holds it either.

That is not an omission to be tidied up later. The evaluator grades the fleet, and the override mechanism above is a directory every pane can read by absolute path and a worker's auto-approve could write into. An override path for that one brief would hand the fleet both the rubric it is judged against and the ability to edit it — and the second is worse than the first, because a grader softened by the generation it judges makes "did the fleet improve" permanently unanswerable rather than merely unfair. **Changing the grader means editing the file in the other repo and rebuilding, deliberately, with a version bump.**

`src-tauri/src/prompts.rs` — the resolver this whole directory goes through — therefore has no evaluator arm, and `src-tauri/tests/evaluator.rs` fails if one appears.

## Editing notes

**Keep each paragraph on one line.** These files are rendered into a system prompt, where a hard wrap becomes a real newline. Prose reads the same either way, but the briefs are checked by tests that read a paragraph as a line — and a peer list split across two lines is harder for a model to parse, not easier. Turn on soft wrap in your editor.

**The sentence that was on loan to WP-07 has been called in (D-051).** `worker.md` used to tell a confused worker to *"ask `orch` to put it to the operator"*; it now says `fleet send operator "<question>"`, and `a_confused_worker_is_told_exactly_who_to_ask` asserts the relay sentence is **gone** rather than merely that the new one is present — two routes to the same person is how a question ends up asked twice or not at all.

**Say what a failure costs, not just what to do.** These briefs are read by `deepseek-v4-flash`, not by Opus. `decisions.md` D-031 records that they are still unvalidated against a live worker — if a worker misbehaves in a way you can name, this directory is the first place to fix it.

## What is *not* here

The three environment settings that each silently wedge a pane forever — the removal of `ANTHROPIC_API_KEY`, the config-dir seed, and the removal of `CLAUDE_CODE_CHILD_SESSION` — are fixed in `src-tauri/src/spawn.rs`. They are listed at the bottom of `launch.conf` with the reason attached, so this directory is still a complete account of what a pane gets. They are documented, not editable: each one fails by looking exactly like a healthy pane while every `fleet send` reports success into a dialog.
