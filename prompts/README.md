# `prompts/` — everything a pane is told

Every word injected into a pane's `claude`, and every flag it is launched with, is a file in this directory. Nothing is spelled out in Rust: `fleetor-core::brief` bakes these in with `include_str!` and fills their placeholders, and `src-tauri::prompts` lets you override them without a rebuild.

For *when* each piece arrives and where it lands inside the pane's context window, see [`docs/context-injection-flow.md`](../docs/context-injection-flow.md).

## The files

| File | What it is | Placeholders it must keep |
|---|---|---|
| `orch.md` | The orchestrator's brief | `{cwd}` `{workers}` `{delivery_contract}` `{scaffolding}` `{vision_tenets}` |
| `worker.md` | The brief every worker slot renders | `{me}` `{cwd}` `{peers}` `{delivery_contract}` `{broadcast_rule}` `{scaffolding}` |
| `delivery-contract.md` | Fragment: what a `fleet` exit code means, and the three outcome words | — |
| `broadcast-rule.md` | Fragment: never answer a broadcast unless it names you | — |
| `scaffolding.md` | Fragment: the working posture CC's own prompt used to supply | — |
| `vision-tenets.md` | Fragment: how the orchestrator thinks about vision | — |
| `launch.conf` | Model, endpoint and flags a worker starts with | — |

All four workers render the same `worker.md`. They differ only in `{me}`, `{peers}` and `{cwd}`.

**These briefs are the pane's *whole* system prompt (D-043).** They go in via `--system-prompt`, which replaces Claude Code's own rather than appending to it. What that does and does not take away is measured in [`docs/system-prompt-notes.md`](../docs/system-prompt-notes.md) — the short version is that CC's guidance leaves and its tools, memory files, skills and git-status section stay. `scaffolding.md` is the part worth restating, and `{cwd}` covers the one thing genuinely lost.

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

Anything else in braces is left alone — these are markdown files, not format strings, so `Vec<{}>` in prose survives.

## Why four of them are fragments

`delivery-contract.md`, `broadcast-rule.md`, `scaffolding.md` and `vision-tenets.md` are composed *into* the briefs at a placeholder rather than written out in them, and a template that drops its placeholder is **refused** rather than rendered.

All three are load-bearing:

- The **delivery contract** is the only reason a model can tell a failed send from a good one. It reads its own Bash exit code and self-corrects. A pane without it silently believes every message arrived.
- The **broadcast rule** is the only mitigation left for broadcast amplification. The hub-side rate limiter was deliberately removed (`decisions.md` D-031) on the grounds that nothing in the delivery path may be able to refuse a message — which is only safe while this clause holds. Five peers that all answer every broadcast is a token fire that looks like a working fleet.
- The **vision tenets** are `docs/futureDesign/vision_tenets.md` distilled to what an orchestrator can act on (D-044). Split out so you can change how the fleet *talks* about vision without editing what it *does* — the rules that bind (confirm in writing before decomposing, propose bigger once, announce deviations) live in `orch.md` and are pinned by tests.
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

## Editing notes

**Keep each paragraph on one line.** These files are rendered into a system prompt, where a hard wrap becomes a real newline. Prose reads the same either way, but the briefs are checked by tests that read a paragraph as a line — and a peer list split across two lines is harder for a model to parse, not easier. Turn on soft wrap in your editor.

**The sentence that was on loan to WP-07 has been called in (D-051).** `worker.md` used to tell a confused worker to *"ask `orch` to put it to the operator"*; it now says `fleet send operator "<question>"`, and `a_confused_worker_is_told_exactly_who_to_ask` asserts the relay sentence is **gone** rather than merely that the new one is present — two routes to the same person is how a question ends up asked twice or not at all.

**Say what a failure costs, not just what to do.** These briefs are read by `deepseek-v4-flash`, not by Opus. `decisions.md` D-031 records that they are still unvalidated against a live worker — if a worker misbehaves in a way you can name, this directory is the first place to fix it.

## What is *not* here

The three environment settings that each silently wedge a pane forever — the removal of `ANTHROPIC_API_KEY`, the config-dir seed, and the removal of `CLAUDE_CODE_CHILD_SESSION` — are fixed in `src-tauri/src/spawn.rs`. They are listed at the bottom of `launch.conf` with the reason attached, so this directory is still a complete account of what a pane gets. They are documented, not editable: each one fails by looking exactly like a healthy pane while every `fleet send` reports success into a dialog.
