# FLEETOR — session orientation

A macOS Tauri 2 app that runs five real, interactive `claude` TUIs against a git repo — one orchestrator (the operator's own Opus) and four DeepSeek Flash workers in worktrees — wired by the `fleet` CLI → unix socket → hub → bracketed-paste pty injection.

This file orients dev sessions working **on** this repo. It does not brush Tier 1.1's "Never `CLAUDE.md`" — that invariant forbids the FLEETOR *app* writing a `CLAUDE.md` into the repo it operates on; fleet panes run in `~/.fleetor/testbed` or the target repo, never here.

## Read first

1. `building.md` §1 — the decision tiers and the eight Tier 1 invariants. The constitution; everything else defers to it.
2. `docs/README.md` — the index of all documentation, by kind. Read it before launching any explore agent.
3. `docs/fleet-comms-map.md` — the message path; §8 is the ordered walk through the code.
4. `docs/roadmap/00-index.md` — the live roadmap and how new work gets filed.

## The system in one breath

Eight `fleet` verbs: `send broadcast reply cmd task done roster whoami`. Nothing may delay, refuse, reorder, drop or alter a message between `fleet send` and the pty (D-034). `accepted` means the bytes reached a live pty — never "delivered"; a message to `operator` is `recorded`. The task board is a diary, not a dispatcher (D-047). Workers merge only to `fleet/integration`, never trunk.

## Commands

```bash
npm run tauri dev                                        # the app (needs claude on PATH + DEEPSEEK_API_KEY in .env)
FLEETOR_PANE_CMD=$PWD/tests/fake-pane/fake-pane.sh npm run tauri dev   # zero-token run, everything real but the agents
cargo test --workspace                                   # workspace tests
cargo test --manifest-path src-tauri/Cargo.toml          # shell tests, 11 of them drive five real ptys
npx tsc --noEmit && npx vite build                       # the frontend check — there is no npm test script
```

## Conventions

- **Branches:** feature branches only, PR to master; never commit to trunk directly.
- **Commits:** conventional-commit prefixes (`feat(cmd):`, `fix:`, `docs:`, `spike:`) with lowercase, declarative prose subjects. Cite decision numbers in the subject (`… (D-045)`) — `git log --oneline | grep D-0NN` is a working index into `decisions.md`.
- **Never add Claude / Co-Authored-By attribution** to commits or PR bodies.
- **Tier 2 change ⇒ `decisions.md` entry** (what changed / why the default lost / what would reverse it). The file is append-only; navigate it with `grep "^## D-" decisions.md`.
- **Spike before building** anything that touches an unknown: throwaway in `examples/`, findings in a version-stamped `docs/notes/<topic>-notes.md`. Where a spike and a plan disagree, the spike wins (building.md §4).
- A verb change moves `prompts/orch.md` + `prompts/worker.md` + `VERBS` + the clap enum + pinned tests **in one commit** — the brief validation refuses a prompt that doesn't teach every verb.
- Prompt edits are re-measured with `examples/system-prompt-spike/count.py`; caps are orch 3,500 / worker 2,200 tokens (D-053/D-056).

## The documentation system

Five kinds of document, five homes — `docs/README.md` is the full account and the per-file index:

| Kind | Home |
|---|---|
| Root logs (constitution, decision log, front door) | `building.md` · `decisions.md` · `README.md` |
| As-built reference | `docs/` |
| Measurement notes (version-stamped evidence) | `docs/notes/` |
| Roadmap work packages | `docs/roadmap/` (+ `roadmap/source/` for raw material) |
| Archive (superseded; banner-flagged; never edited below the banner) | `docs/archive/` |

The maintenance loop — follow it, it is what keeps the next session from needing explore agents:

- **A feature lands →** update the as-built doc that covers it in the same PR (message path → `fleet-comms-map.md`; a new verb → also README's verb list). Tier 2 choices get their D-entry.
- **A work package closes →** tick its exit checklist, set its frontmatter `status: landed`, append a short "How it landed", and update `docs/roadmap/00-index.md`'s status column — the last act of the session.
- **A spike runs →** `docs/notes/<topic>-notes.md`, version-stamped, plus an index row in `docs/README.md`.
- **Future work is conceived →** file it as a package from `docs/roadmap/TEMPLATE.md`: next number, index row, graph edge. Even doc-writing work — WP-10 is one.
- **A doc stops being true →** archive it with a banner, never patch history; repoint live references; move its index row.
- **Never duplicate a fact that has a canonical home** — link it.

## Traps

- `.claude/worktrees/*` can hold stale checkouts — grep hits there are old copies of these files.
- `docs/CLAUDE.template.md` is the operator's personal untracked file. Never edit, move, or commit it.
- The three env settings in `src-tauri/src/spawn.rs` each silently wedge a pane forever if wrong (`prompts/README.md` names them). Workers must never see `ANTHROPIC_API_KEY`.
- Live-spend runs (real Opus/DeepSeek tokens) are named and operator-approved before they happen (building.md §9.5).
