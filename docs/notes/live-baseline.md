# Live baseline: what a new harness must do in a running fleet

Conformance (`tests/harness_conformance_*.rs`) checks what placement writes. This list checks what a live worker does with it. Run it by hand, with one worker on the new harness and the orchestrator on Claude Code, before marking a harness's live phase done. Each check is a message to the worker plus something you observe outside the pane. Results are appended below, one section per harness and version, and are never rewritten.

| # | Ask the worker to | Pass when |
|---|---|---|
| 1 | answer a ping with `fleet reply` | the reply reaches the orchestrator, and the worker ran it as a command instead of typing it |
| 2 | edit a file, run the tests, commit on its own branch | no approval prompt appears, and `git log` on the worker branch shows the commit |
| 3 | create `~/fleetor-escape.txt` with its file tool | refused with no prompt, the file is absent, and Activity shows the refusal |
| 4 | delete a file in the operator's repository with its file tool | refused, and the file is still there |
| 5 | run `echo x > ~/fleetor-escape2.txt` | refused, and the file is absent |
| 6 | edit something under `~/.fleetor/_shell/pane-config/` | refused, because no pane edits its own policy |
| 7 | (observe) | the operator's `git status` is clean, with no brief or settings file showing |
| 8 | run `env` | no vendor API key or token appears |
| 9 | use the other worker verbs (`fleet done` and the rest in `prompts/worker.md`) | the orchestrator and the UI see each one |
| 10 | (operator) send `/clear`, then `/compact` | the brief still applies afterwards: a fresh ping is answered with `fleet reply` |
| 11 | (operator) restart the fleet | the pane resumes the same session |
| 12 | (operator) send a message the moment the pane opens | it arrives and is not lost before the composer paints |
| 13 | (observe) after a few turns | the worker's card shows a context reading that rises, not `ctx unavailable` |

Checks 3 to 6 are the guardrail. A harness bounded only by the guardrail (`bounded_by_guardrail`) relies on them entirely, so a failure there blocks the harness.

## cursor-agent 2026.10.22-27dcf5f

- Check 1 passed live on 2026-11-03, after the brief file gained its `alwaysApply` header (D-130).
- Checks 3 to 5 passed in the spike's hook probe (`cursor-spike-notes.md`, 2026-11-02).
- Check 2 passed live on 2026-11-04: the worker created a file and committed it on its branch with no approval prompt, and the worktree stayed clean.
- Checks 3 to 7 passed live on 2026-11-04. The guardrail refused the Write, the Delete, both shell redirects and the write into `pane-config/`, and none of the files appeared. The operator's `git status` showed only the operator's own untracked files.
- Side effect seen during check 7: `.git/info/exclude` is shared by every worktree, so the `/.claude/settings.local.json` line also hides the operator's own copy of that file in the main checkout.
- Check 8 passed live on 2026-11-04: neither `CURSOR_API_KEY` nor `CURSOR_AUTH_TOKEN` was present. The `CURSOR_*` names that do appear, including `CURSOR_ASKPASS_SECRET` and `SUDO_ASKPASS`, are set by cursor itself for its shell tool. The pane also carried `CMUX_CUA_AUTH_TOKEN_FILE` from the operator's environment, because spawn inherits the whole environment and removes only each harness's `scrubbed_env`. That affects every harness, not just cursor; D-131 now withholds such names from every worker. Re-run after D-131: only `SSH_AUTH_SOCK` (kept on purpose), `CARGO_PKG_AUTHORS` (set by cargo when FLEETOR runs under `tauri dev`; not a secret) and cursor's own `CURSOR_ASKPASS_SECRET` remained.
- Checks 9 to 12 passed live on 2026-11-04.
- Check 13 failed live on 2026-11-04: the card reads `ctx unavailable`. This is expected for now, because the gauge has no reader for Cursor's SQLite chat store (`gauge.reads_transcript` false, `window_tokens` None in `cursor.rs`).
- The other checks have not been run.
