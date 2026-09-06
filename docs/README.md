# docs/ — the index

The map a fresh session reads instead of launching explore agents. Every documentation change updates this file as its last act — a doc that isn't listed here doesn't exist.

## The taxonomy — five kinds of document, five homes

| Kind | Home | Mutability |
|---|---|---|
| **Root logs** — the constitution, the decision log, the front door | `building.md` · `decisions.md` · `README.md` (repo root) | building.md surgically; decisions.md **append-only entries**; README kept current |
| **As-built reference** — what the code does now | `docs/*.md` | updated in the same PR as the change it describes |
| **Measurement notes** — spike evidence, version-stamped | `docs/notes/` | append findings; **never rewritten** — where a note and a plan disagree, the note wins (building.md §4) |
| **Roadmap** — future work as executable work packages | `docs/roadmap/` | statuses move `not-started → landed`; "How it landed" appended at close |
| **Archive** — superseded systems and moments that passed | `docs/archive/` | banner-only edits; never edited below the banner, never deleted |

## As-built reference (`docs/`)

| File | The question it answers | Status |
|---|---|---|
| [`fleet-comms-map.md`](./fleet-comms-map.md) | How does a message travel from `fleet send` to a terminal? §8 is the code walk. | current |
| [`context-architecture.md`](./context-architecture.md) | *What* is a pane told, and who decided each piece? | current |
| [`context-injection-flow.md`](./context-injection-flow.md) | *When* does each piece of context arrive, and where does it land in the window? | current |
| [`runtime-layout.md`](./runtime-layout.md) | What lives under `~/.fleetor` and what bites? | current |

`prompts/README.md` (beside the prompt files, not here) is the account of the briefs themselves. The `ui/` frontend has no map yet — that is WP-10 on the roadmap.

## Measurement notes (`docs/notes/`)

Each is stamped with the Claude Code version it measured; re-measure on a CC update (the risk register names this).

| File | What it measured | Stamp |
|---|---|---|
| [`tui-spawn-notes.md`](./notes/tui-spawn-notes.md) | What an interactive `claude` pane needs before it reaches a prompt | CC 2.1.220 |
| [`system-prompt-notes.md`](./notes/system-prompt-notes.md) | What `--system-prompt` removes vs `--append-system-prompt` (D-043) | CC 2.1.223 |
| [`command-channel-notes.md`](./notes/command-channel-notes.md) | What a pasted `/clear` / `/compact` actually does — empty box, queued text, mid-turn (D-045) | CC 2.1.223 |
| [`context-gauge-notes.md`](./notes/context-gauge-notes.md) | The worker transcript path and usage schema the gauge reads (D-046) | CC 2.1.223 |
| [`peer-review-notes.md`](./notes/peer-review-notes.md) | Plain-git proof a peer can review a branch from its own worktree (D-048) | git only |
| [`fence-notes.md`](./notes/fence-notes.md) | What a private worker HOME stops, and the breakage catalogue (D-052); the Rust toolchain arms (D-069) | 2026-08-06; toolchain arms 2026-08-08 |
| [`blackboard-shakedown.md`](./notes/blackboard-shakedown.md) | The WP-09 live-run findings | **live log** — CC 2.1.223, ongoing |
| [`run-rotation-notes.md`](./notes/run-rotation-notes.md) | Which files an archived run has to take, measured against a crashed WAL (D-058) | 2026-08-07, SQLite/macOS 15 |
| [`orch-config-dir-notes.md`](./notes/orch-config-dir-notes.md) | Why a fleet-owned `CLAUDE_CONFIG_DIR` silently logs `orch` out, and the variable that keeps its login (D-062) | CC 2.1.224 |
| [`write-guardrail-notes.md`](./notes/write-guardrail-notes.md) | That a `PreToolUse` deny really stops `Bash`, and where a real build and a real commit actually write (D-065) | CC 2.1.224 |
| [`live-run-snapshot-notes.md`](./notes/live-run-snapshot-notes.md) | That a run still being written reads whole through a read-only connection, and that copying `state.db` alone loses it (D-066) | 2026-08-07, sqlite3 3.43.2 |
| [`critic-spike-notes.md`](./notes/critic-spike-notes.md) | Whether a judge with no answer key finds anything worth reading, and the prompt that became the Critic's brief (WP-20 D15) | 2026-09-02, archives CC 2.1.224 |
| [`codex-spike-notes.md`](./notes/codex-spike-notes.md) | What codex honours as a brief, what its sandbox refuses, and the one key that reaches the hub (C3, C5, C7, C22) — **re-runnable**: `examples/codex-spike/probe.py` | `codex-cli 0.153.4` |

## Roadmap (`docs/roadmap/`)

[`00-index.md`](./roadmap/00-index.md) is the hub: package table with statuses, dependency graph, contention warning, standing tensions. To file new work: copy [`TEMPLATE.md`](./roadmap/TEMPLATE.md) to the next zero-padded number, fill it in, add a row and a graph edge to `00-index.md`. Statuses in the index move **as the last act of the session that lands a package**.

[`12-self-improving-loop.md`](./roadmap/12-self-improving-loop.md) is an **arc doc**, not a
package: it carries the self-improvement design (both flow diagrams, the hiding model, the
four invariant arguments) and files WP-13..19, each written from `TEMPLATE.md` when picked
up. The rewind harness it names lives in a separate repo by design.

[`16-dev-mode.md`](./roadmap/16-dev-mode.md) is the first of those to land (D-061). It is
also the as-built record for dev mode: where the flag lives, the one function that reads
it, and the two tests that keep the mode out of the delivery path (Tier 1.4) and out of
every word a pane is told (WP-12's open question 4).

[`17-write-guardrail.md`](./roadmap/17-write-guardrail.md) landed the write guardrail
(D-065) and is also its as-built record: the per-pane roots, the `PreToolUse` hook that
enforces them, what it deliberately does not stop, and why narrowing auto-approve does not
trip Tier 1.7. It is the package WP-12 puts between an auto-approving worker and the
evaluator's own code, so it blocks any improve run.

[`13-done-verb.md`](./roadmap/13-done-verb.md) landed the ninth `fleet` verb (D-064):
`fleet handoff`, by which `orch` declares the confirmed goal met. The message path's
account of it is [`fleet-comms-map.md`](./fleet-comms-map.md) §3e; the package doc carries
why the verb is not called `done`, why it answers `recorded`, and the nine places a verb
list is written down.

[`15-evaluator-window.md`](./roadmap/15-evaluator-window.md) landed the evaluator (D-066)
and is the as-built record for it: the `PaneId` that is in no enumeration, the wake that
rides the event bus rather than the message path, how a run that is still being written
gets read, and the `devmode` cargo feature that compiles the grader's brief in from a
separate repo. Read its section **"The one test whose letter changed"** before touching
`tests/dev_mode.rs` — it is the one existing tripwire this arc rewrote, and why the
replacement is stricter. The message path's account is
[`fleet-comms-map.md`](./fleet-comms-map.md) §3f, which is written to be read beside §3c.

`roadmap/source/` is the raw upstream material — the operator's vision-tenets essay (`vision_tenets.md`; the shipped copy is `prompts/vision-tenets.md`, D-056) and the "Autonomy Designs" catalogue (`fleetor-autonomy.html` and `fleetor-autonomy-plain.html` — two renders of one document).

## Archive (`docs/archive/`)

Kept as the record behind decisions still in force, or as handoffs that were acted on — never as a description of the code. Each opens with a banner saying what superseded it.

| File | What it was |
|---|---|
| `handoff.md` | The pre-pivot headless architecture (MCP shim, supervisor, ticket board) — all deleted by D-030 |
| `phase2-spikes.md` | MCP-handshake and Stop-hook spikes; both mechanisms deleted (D-030), D-014 survives |
| `phase0-report.md` + `.json` | DeepSeek Flash tool-call fidelity through headless CC — mechanism gone, the finding is why workers run Flash |
| `tui-pivot-plan.md` | The executed pivot plan, Phases 0–6, deviations recorded inline |
| `MORNING-HANDOFF.md` | The overnight Blackboard build's handoff to the operator |
| `OVERNIGHT-QUESTIONS.md` | Q-1..Q-4 — unattended judgment calls, all closed by measurement |
| `prompt-budget-menu.md` | Six costed prompt-cut candidates; overtaken by D-054/D-056 |

## Adding or updating a doc

1. Decide its kind from the taxonomy table — that decides its home.
2. Notes are new files (`docs/notes/<topic>-notes.md`, version-stamped); as-built docs are edited in place, in the same PR as the code change; roadmap items come from `TEMPLATE.md`.
3. A doc that stops being true is **archived, not fixed**: move it to `docs/archive/`, add the banner (what superseded it, where current truth lives), repoint every live reference.
4. Add or update this file's row for it. Last act, every time.

Never write the same fact in two places — link to its canonical home instead. The stale-budget-numbers episode (one measurement quoted in four docs, three of them wrong within a day) is why.

## Warnings

- `.claude/worktrees/*` may contain stale checkouts of this repo — grep hits under it are old copies, not the live docs.
- `docs/CLAUDE.template.md` is the operator's own untracked file. Never edit, move, or commit it.
