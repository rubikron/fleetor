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
| [`fence-notes.md`](./notes/fence-notes.md) | What a private worker HOME stops, and the breakage catalogue (D-052) | 2026-08-06 |
| [`blackboard-shakedown.md`](./notes/blackboard-shakedown.md) | The WP-09 live-run findings | **live log** — CC 2.1.223, ongoing |
| [`run-rotation-notes.md`](./notes/run-rotation-notes.md) | Which files an archived run has to take, measured against a crashed WAL (D-058) | 2026-08-07, SQLite/macOS 15 |

## Roadmap (`docs/roadmap/`)

[`00-index.md`](./roadmap/00-index.md) is the hub: package table with statuses, dependency graph, contention warning, standing tensions. To file new work: copy [`TEMPLATE.md`](./roadmap/TEMPLATE.md) to the next zero-padded number, fill it in, add a row and a graph edge to `00-index.md`. Statuses in the index move **as the last act of the session that lands a package**.

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
