# codex token-usage notes

**Version-stamped: `codex-cli 0.153.4`.** Re-runnable: `examples/codex-spike/usage_probe.py`.

This note closes C12's one named open spike. Unlike every other note in this
directory, the measurement behind it **spent real money** — see [What it cost](#what-it-cost).
`--reuse` re-reads the run for free.

## The question

Does codex persist per-turn token usage in its thread store at all?

C12 measured the store's schema — `thread_items` and `thread_turns` in a
generation-numbered, `_sqlx_migrations`-managed `thread_history_1.sqlite` — but could
not measure usage, because the probe behind it ran against a capture server that
answers `data: [DONE]` and so **never completed an assistant turn**. No completed turn,
no usage row to look for. The zero-token tier cannot answer this one, which is why this
is the one arm in WP-25 authorised to spend.

## The answer

**No.** The thread store C12 measured does not persist token usage — not as a column,
and not inside any `item_json`.

**But codex does persist it, twice, elsewhere in the same `CODEX_HOME`** — and both are
numbers the vendor wrote, not numbers anyone reconstructed. That distinction is the
whole finding, and it is what changes what #41 does.

| Where | What it holds | Granularity |
| --- | --- | --- |
| `thread_history_1.sqlite` (C12's store) | **nothing** | — |
| `state_5.sqlite` → `threads.tokens_used` | a running total for the thread | per thread |
| the rollout JSONL → `token_usage_record` | the full breakdown | **per turn** |

`state_5.sqlite`'s `threads` row also carries `rollout_path`, an absolute path to that
thread's JSONL — so the store, the total and the per-turn breakdown are joinable by
`thread_id` with no guessing.

## Method

One `codex exec` turn, prompt `Reply with exactly: ok`, `model_reasoning_effort="low"`,
`sandbox_mode="read-only"`, `approval_policy="never"`.

**Per C47 the vendor binary was resolved absolutely, and this is the record of which
one the money was spent through.** Not the `codex` on PATH — that is a cmux shim which
injects six `-c hooks.*` flags pointing at `~/.cmux/hooks/*.sh`, and its
`UserPromptSubmit`/`PreToolUse`/`Stop` hooks would have run around the measuring turn,
making the reading one of the vendor *plus* cmux. Not `/opt/homebrew/bin/codex` either,
which is a Node wrapper. The turn was spent through the native binary:

```
/opt/homebrew/lib/node_modules/@openai/codex/node_modules/
  @openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex
```

Confirmed the way #34 confirms it — that binary's own
`runtime.provenance.details["current executable"]` reports **that same path**, and the
probe prints the pair on every run so the reading stays re-checkable. The fabricated
`config.toml` contains no `[hooks]` table (`grep -c hooks` → 0) and the binary was
invoked with explicit argv, so nothing was injected into the measuring turn.

**The operator's real `~/.codex` was never written to**, as in every earlier arm. Its
`config.toml`, `models.json` and `auth.json` were *copied* into a fabricated
`CODEX_HOME` at `/tmp/codex-usage-spike/home`, the catalog pointer rewritten to the
copy, the operator's `[projects]` trust entry stripped and replaced with one for the
scratch worktree. The turn ran entirely inside that home.

## The evidence, literally

**The thread store, after a completed turn.** Both tables' full column lists, and every
persisted row:

```
thread_turns: thread_id, turn_id, rollout_ordinal, status, error_json, started_at,
              completed_at, duration_ms, first_user_item_id, final_agent_item_id,
              rollout_byte_offset, rollout_end_ordinal, rollout_end_byte_offset
thread_items: thread_id, turn_id, item_id, rollout_ordinal, created_at_ms,
              item_json, item_type, updated_at_ordinal
item_type counts: {'userMessage': 1, 'reasoning': 1, 'agentMessage': 1}
```

No usage column on either table. All three `item_json` documents walked for any key
matching `usage|token`: none. The blunt form of the same query returns zero:

```sql
select count(*) from thread_items
 where item_json like '%token%' or item_json like '%usage%';   -- 0
```

**And it is not an accident of this one turn.** Every `CREATE TABLE`/`ALTER TABLE`
statement the vendor binary ships for these two tables — seven of them, recovered with
`strings` from the binary itself, so this arm costs nothing — was grepped for a usage
column. Zero hits. The columns later migrations add are `item_type`,
`updated_at_ordinal`, `rollout_byte_offset`, `rollout_end_ordinal`,
`rollout_end_byte_offset`. There is no migration in this build under which the store
would hold usage.

**`state_5.sqlite`, same home, same run:**

```
threads.tokens_used  = 12548
threads.rollout_path = …/sessions/2026/09/06/rollout-…-01a0779e-….jsonl
```

**The rollout JSONL, same home, same run** — one `token_usage_record` for the one turn,
verbatim:

```json
{"type":"token_usage_record","payload":{
  "thread_id":"01a0779e-9ae7-7181-ad8f-596038c2a862",
  "turn_id":"01a0779e-9af8-7903-a7c8-e00ae99ae884",
  "usage":{"input_tokens":12533,"cached_input_tokens":0,"cache_write_input_tokens":0,
           "output_tokens":15,"reasoning_output_tokens":13,"total_tokens":12548},
  "turn_token_usage":{…same…},"thread_token_usage":{…same…}}}
```

and, one line later, the `token_count` event carrying `TokenUsageInfo`:

```json
{"type":"event_msg","payload":{"type":"token_count","info":{
  "total_token_usage":{…,"total_tokens":12548},
  "last_token_usage":{…,"total_tokens":12548},
  "model_context_window":996147}}}
```

`12548` is the same number codex's own run output printed as `tokens used`.

## The denominator does not match the catalog — read this before building the gauge

The catalog publishes `context_window = 1048576` for all three of the operator's models.
codex's own `token_count` event reports `model_context_window = 996147` for the same
model in the same run. That is **exactly 95%** of the catalog figure.

So there are two defensible denominators and they differ by 5%. A gauge that divides by
the catalog's `1048576` will not agree with what codex's own TUI shows the operator.
**Recommendation: use the `model_context_window` codex reports in `token_count`, and
fall back to the catalog's `context_window` only when no `token_count` has been seen** —
the rail should agree with the vendor's own display rather than with the vendor's own
catalog. This is a real discrepancy in the vendor, not an artifact of the fabricated
home; it is recorded here rather than resolved, because it is #41's call to make.

## What this means for the gauge (#41)

**The rail does not have to read `unavailable`.** C24's out-of-scope entry and M22's
fallback both refuse *a number synthesized from what FLEETOR sent* — an estimate.
Neither refuses reading a number the vendor itself wrote to disk. `token_usage_record`
is the vendor's own accounting of its own turn; reading it is the same class of act as
reading `thread_items` for the transcript, which checkpoint 13 already does.

**No reconstruction is proposed here, and none should be.** If a thread has no
`token_usage_record` — a turn that errored before the response, a rollout that has not
been flushed, a build that stops writing them — the rail reads **unavailable** for that
thread. It never derives a figure from the prompt FLEETOR assembled. That rule is
unchanged.

**One thing #41 must not assume.** C54's harvest takes the *thread store* by
`VACUUM INTO`, and the rollout JSONL is a different file that the harvest does not
currently touch. Reading usage therefore means a **second source** on the codex path.
That is a scope question for #41, not a fact this spike settles: the fact settled here
is only that the number exists, where it lives, and that it is the vendor's.

## What it cost

**One turn. 12,548 tokens** — 12,533 input, 15 output (13 of them reasoning), on
`deepseek-v4-flash`. Wall clock 2.4s. The probe's other seven arms cost nothing: two run
before the turn against the catalog and the binary, five read what the turn left on disk.

**The dollar figure was not measured.** The intended method — reading the provider's
balance endpoint immediately before and after — was blocked, so no independent
before/after reading exists. **Believed, not verified:** at any current flash-tier rate
12.5k tokens is well under one US cent. The token count above *is* measured, and is the
number to re-price against if that ever matters.

No second turn was run. The re-verification after the probe was rewritten used
`--reuse`, which re-reads the same scratch home and contacts nothing.

## Re-running it

```
python3 examples/codex-spike/usage_probe.py            # banner only, refuses, spends nothing
python3 examples/codex-spike/usage_probe.py --reuse    # re-reads the last run, free
python3 examples/codex-spike/usage_probe.py --spend    # ONE real turn, real money
```

Eight arms, PASS/FAIL, exit code is the failure count. It refuses to spend without
`--spend` (or `FLEETOR_SPEND_OK=1`) precisely so that nothing in CI can bill anyone by
running the directory. Arm `no-usage-in-store` fails **in either direction** — if a
later build starts persisting usage in the thread store, this file is what notices.
