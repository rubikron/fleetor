# Codex resume spike — findings

Binary probed: `codex` v0.153.4 (via cmux-cli-shim, which forwards real CLI help).

## Command surface (VERIFIED this session, from `codex resume --help` / `codex fork --help`)

- `codex resume [SESSION_ID]` — **accepts a session id (UUID) or session name directly**; UUID takes precedence. `--last` continues most recent, `--all` disables cwd filtering. So resume-invocation for the Codex harness (06) = `codex resume <uuid>`.
- `codex fork [SESSION_ID]` — **native fork by UUID**: forks a previous session into a new one. Parallel to `claude --fork-session`.
- Session management subcommands exist: `codex archive` / `delete` / `unarchive` (by id or name). Fleetor manages runs at its own level, but these confirm sessions are addressable by id — relevant context for delete (11).

## Artifact question — the PRD's Codex-both assumption likely FLIPS (help-text evidence, needs runtime confirm)

The PRD assumed the current `SqliteBackup` archive of `thread_history_1.sqlite` may be insufficient because `codex resume` replays the `sessions/.../rollout-*.jsonl`, so it planned to capture both.

New evidence from this version's help:
- `codex migrate-rollouts` — "Inspect or migrate **legacy** local sessions to **paginated thread history**." This frames rollout jsonl as the **legacy** format and thread-history (the sqlite) as the **current** one that resume reads.
- `codex resume <UUID>` addresses sessions by id against thread history.

**Tentative conclusion:** on codex ≥ this version, archiving `thread_history_1.sqlite` (what we already do) is likely **sufficient** to resume, and capturing the rollout jsonl may be unnecessary. This *de-risks* Codex and may shrink ticket 06.

**NOT yet proven.** Requires the runtime test: point `CODEX_HOME` at a restored home containing only the archived sqlite, run `codex resume <uuid>`, and confirm it resumes without the rollout present. Until then, 06 keeps "capture both" as the safe default.

## Token behavior on load
- Not measured. Needs a live launch under a restored `CODEX_HOME` with network watch. Gated to 06, not 03.

## Open items (live probe)
- Prove sqlite-only restore resumes (the artifact question) → decides whether 06 captures one artifact or two.
- Confirm session-id source: where fleetor reads the resumable UUID at archive time (manifest/session file under `CODEX_HOME`).
- Load-time token behavior.
- Whether resume/fork require the recorded cwd to match (codex resume filters by cwd unless `--all`).

## RESOLVED (ticket 06 live shakedown, codex 0.153.4) — WP-26 C9

- **`codex resume` opens on an interactive "Choose working directory to resume this session" picker** (options: use session dir / use current dir / always-use-either / "Press enter to continue"). It fires **unconditionally**, not only on cwd mismatch — verified by running resume with the launch cwd set equal to the recorded session cwd (still prompts) vs. a different cwd (still prompts).
- A **read-only viewer can never answer it** (the fence eats Enter), so the pane hangs on the picker. This was the live blocker for codex seats in View, not the transcript restore (which works).
- **Fix: `-c tui.resume_cwd=current`.** The config key `tui.resume_cwd` (found in the real codex binary `~/.local/bin/codex`; the PATH `codex` is a cmux shell shim that no-ops outside a cmux terminal) pre-answers the picker. `current` (the viewer's own ephemeral cwd) over `session` because the recorded repo may be gone; cwd is immaterial to a read-only render. Now in `codex.rs::resume_args`, pinned by a codex test.
- Sqlite-only restore (no rollout jsonl) **does** drive resume: the restored `thread_history_*.sqlite` store alone reached the working-dir picker (i.e. resume found and loaded the session), confirming the spike's tentative "sqlite is sufficient" conclusion for this version.
