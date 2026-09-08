# Reopening a run from local logs — spike notes

Measured 2026-09-08 against **`2.1.263 (Claude Code)`** and **`codex-cli 0.153.4`**, macOS 15 (Darwin 24.6.0).
Re-runnable: `python3 examples/reopen-spike/probe.py --spend` (creates the sessions), `--reuse` (re-reads them for free).

For WP-27 (`docs/roadmap/27-reopening-a-run.md`), decisions R3/R4/R6/R13.

**Both vendor binaries must be resolved absolutely.** C47 recorded that the PATH `codex`
is a cmux shim; on this machine the PATH `claude` is one too
(`/var/folders/.../cmux-cli-shims/.../claude`). A probe taking either from PATH measures
cmux plus the vendor. The real ones: `~/.local/share/claude/versions/2.1.263` and the
C47 codex path.

## §1 — The headline: neither vendor forks on resume

**This is the finding WP-27's R3, R4 and R6 rest on, and both harnesses agree.**

| | Claude Code | codex |
|---|---|---|
| where the session lives | `<CLAUDE_CONFIG_DIR>/projects/<cwd-slug>/<id>.jsonl` | `<CODEX_HOME>/thread_history_1.sqlite` |
| the resumable id | the filename stem | `thread_items.thread_id` |
| reopen verb | `claude --resume <id>` | `codex resume <id>` |
| **does a resumed turn fork?** | **no** | **no** |

- **Claude Code.** A resumed turn appended to the *same file*: 11,857 → 16,185 bytes,
  `user` rows 1 → 3, `assistant` rows 1 → 3, **no new `.jsonl` created**, and every row in
  the file still carries one single `sessionId` equal to the filename stem.
- **codex.** A resumed `exec` appended to the *same thread*: `thread_items` for that
  `thread_id` went 3 → 6 with **zero new threads** in the store.

So R3's "continue in place, no copy" and R4's per-run directory are both mechanically
sound, and R6's per-seat `session_id` is a stable handle on both harnesses. `--fork-session`
(Claude Code) and `codex fork` exist but are opt-in; neither is reached by a plain resume.

## §2 — Claude Code's session id *is* its filename, proven from inside the file

`projects/-private-tmp-fleetor-reopen-spike-cc-cwd/178043fa-4575-467f-868f-e80a473ba704.jsonl`
— all 10 rows carry `sessionId = 178043fa-4575-467f-868f-e80a473ba704`. The id is not
inferred from the name; the name and the contents agree. Row types seen:
`user`, `assistant`, `attachment`, `queue-operation`, `last-prompt`, `atis-latch`, `mode`.

## §3 — The trust key must name the **realpath**, and this is how a reopened pane hangs

D-030 recorded that `projects[cwd].hasTrustDialogAccepted` is keyed by absolute path.
**Measured addition: it is keyed by the *resolved* path.** Seeding the key for
`/tmp/…/cwd` while the process resolves to `/private/tmp/…/cwd` left the reopened pane
sitting on a full-screen gate it could never be sent past:

```
Quick safety check: Is this a project you created or one you trust?
❯ No, exit
  Yes, I trust this folder
Enter to confirm · Esc to cancel
```

Seeding the key against `os.path.realpath(cwd)` cleared it and the session rendered.
**Relevance to WP-27:** R4 gives every run a *fresh* seat directory, so this seeding runs
on a cold config dir every time — the exact condition that raises this gate. Note also
that this is 2.1.263's wording; earlier notes say "Do you trust the files", so any
wall-detection that matches on prose needs both.

## §4 — codex's working-directory picker is cwd-DEPENDENT (corrects a carried claim)

WP-27 carried, from an earlier shakedown, that stock `codex resume` "fires unconditionally
on this version" and that "matching the launch cwd to the recorded one does *not* suppress
the prompt". **That is wrong at 0.153.4.** Measured both ways against the same session:

- **Launch cwd == the session's recorded cwd** → no picker. `Resuming session…`, then the
  session's own turns, straight through.
- **Launch cwd != recorded cwd** → the picker fires:

  ```
  Choose working directory to resume this session
  Session = latest cwd recorded in the resumed session
  Current = your current working directory
  › 1. Use session directory (/private/…/cx/codex-cwd)
    2. Use current directory (/private/…/cx/elsewhere)
    3. Always use session directory
    4. Always use current directory
  Press enter to continue
  ```

- `-c tui.resume_cwd=current` suppresses it in the case that raises it.

**Why the earlier claim overgeneralized:** that shakedown drove a *viewer* whose cwd was an
ephemeral `view/<cell>/cwd` by design, so its launch cwd could never match and it only ever
observed the firing case.

**What this means for WP-27.** R7 puts a reopened worker back in its own worktree and orch
in the recorded target, so cwds normally match and the picker never fires. Keep the override
anyway, as a **defence for the mismatch case** rather than an unconditional necessity: a
worktree recreated at a different path would raise a gate that, under R13 (nothing is
injected and no key is sent), no one would answer — and the pane would hang exactly the way
§3's trust gate does.

## §5 — Two facts about driving these TUIs, both learned the hard way

**Never write to a pty master during teardown.** The first probe sent `^C` before killing
the child and hung for **eleven minutes on a twelve-second deadline** — a write to a master
whose child has stopped reading blocks forever, leaving the child a zombie and `waitpid`
unreached. Signal the child's process group (`pty.fork` makes it a session leader) and reap
with `WNOHANG`. Waking the pane *while it is alive and reading* is fine and is what the
product already does.

**A pty with no window size renders nothing.** `pty.fork` leaves the size at 0×0. Claude
Code coped; codex painted only its terminal-mode preamble — 12 characters — which a naive
"did it render?" arm reads as a blank screen and a naive "is the picker gone?" arm reads as
*good news*. Set `TIOCSWINSZ` before the child draws.

**codex needs its wake, and this is `BringUp::AfterWaking` earning its place.** Without
`WAKE_KEY` (`pty.rs:101`, `b"\r"`) the render captures 32 KB of splash ASCII art and no
session. A probe that scanned that for picker words would have reported "no picker" from a
screen that was never the session.

## §6 — What was NOT measured, and why

- **A live model turn through a fenced Claude Code config dir.** Every `claude` turn here
  returned `Not logged in · Please run /login`, including the first, so **the Claude Code
  half of this spike spent nothing**. The credential lives in the macOS keychain (C49) and
  the product writes it into each fenced dir itself (`harness.rs` `write_operator_login` —
  "a fenced pane has no login keychain on its search list"); the probe does not, and
  reading the operator's keychain would prompt them to prove something WP-27 already
  relies on. **Consequence for §1:** Claude Code's no-fork result was measured with turns
  that failed at the model, so it proves the *session plumbing* appends rather than forks,
  not that a successful assistant turn does. codex's equivalent arm ran with real replies
  (`• PELICAN-7731`, twice) and agrees, which is the reason to believe it — but a
  credential-seeded re-run would close it properly.
- **Whether a resumed pane's later transcript still lands where the gauge looks.** Not
  driven; `context_gauge::latest_transcript` picks by mtime, and both harnesses appended in
  place, so it should hold — unverified.
- **codex trust.** `codex exec` refuses outside a git repo (`Not inside a trusted directory
  and --skip-git-repo-check was not specified`); the probe passes `--skip-git-repo-check`,
  which is **exec-only** and rejected by `resume`. The product handles this through
  checkpoint 14 (`codex-trust-key-notes.md`), which was not exercised here.
