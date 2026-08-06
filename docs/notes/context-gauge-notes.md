# A worker's own transcript — path, schema, and what actually updates

Measured against **Claude Code 2.1.223** on macOS 24.6.0, via
`examples/context-gauge-spike/probe.py`, one live run on `deepseek-v4-flash`.
The fourth of the version-stamped measurement docs, after `tui-spawn-notes.md`
(config keys), `system-prompt-notes.md` (what `--system-prompt` takes away) and
`command-channel-notes.md` (what a pasted slash command does). Same rule:
**everything below is verified by running it**, and where this disagrees with
a plan, this wins (`building.md` §4).

The question WP-04 had to answer before building a gauge: is there a per-worker
file the fleet can read, read-only, to answer "how much of its context window
has worker-2 used" — without adding anything to the message path and without
touching the operator's own `~/.claude`? And if so, which field is the honest
number, given WP-02 already found that Claude Code does not recognize
`deepseek-v4-flash` and silently assumes a 200k window for its own bookkeeping?

---

## 0. How this was measured

One live run, the worker posture from `src-tauri/src/spawn.rs`: isolated
`CLAUDE_CONFIG_DIR`, DeepSeek Flash via `ANTHROPIC_BASE_URL`/`ANTHROPIC_AUTH_TOKEN`,
`ANTHROPIC_API_KEY` removed, `--permission-mode auto`, `--system-prompt` set to
a short brief — the same five things every real worker pane launches with. Two
arithmetic questions were pasted a bounded time apart (`what is 12 + 30?`, then
`now what is that number times 3?`) so the transcript would contain two
distinct assistant turns to diff against each other, then the whole
`CLAUDE_CONFIG_DIR` was walked and every `*.jsonl` found was dumped and
inspected. Cost: one short Flash session, well under a cent.

```console
$ cd examples/context-gauge-spike
$ python3 probe.py --seconds 75
```

Run output (the isolated config dir and a dump of every file found under it)
lands in `work/`, gitignored like the other spikes.

---

## 1. The path

```
<CLAUDE_CONFIG_DIR>/projects/<slug>/<session-uuid>.jsonl
```

`<slug>` is the pane's **absolute cwd** with every `/` and every `.` replaced
by `-`. Verified character-for-character, not eyeballed:

```
cwd      /Users/…/blackboard/examples/context-gauge-spike/work/target-live
slug     -Users-…-blackboard-examples-context-gauge-spike-work-target-live
```

`"".join("-" if c in "/." else c for c in cwd)` reproduces the observed
directory name exactly. This is the same absolute path
`src-tauri/src/spawn.rs::project_key` already canonicalizes for the
`.claude.json` trust-flag key, so the sampler reuses that function rather than
re-deriving the canonicalization rule a second time.

**Two decoys sit next to it and are not the transcript:**

- `<CLAUDE_CONFIG_DIR>/history.jsonl` — a flat per-command index
  (`display`, `pastedContents`, `project`, `sessionId`, `timestamp`). No
  `usage` anywhere in it.
- `<CLAUDE_CONFIG_DIR>/sessions/<pid>.json` — a small per-process marker file,
  not a transcript.

A worker's own `projects/<slug>/` directory can hold more than one `*.jsonl` —
nothing was done in this session to trigger a second one, but `/clear`
starting a fresh session is exactly the kind of thing that would (untested
here; see §4). The sampler does not assume there is exactly one file.

---

## 2. The schema

Newline-delimited JSON, one object per line, discriminated by `type`. The
16-line transcript from two user turns carried eight distinct types:

| `type` | Count | What it is |
|---|---|---|
| `mode` | 1 | Session-start marker |
| `permission-mode` | 1 | Echoes `--permission-mode auto` |
| `system` | 3 | `subtype: informational` (the auto-mode notice, once) and `subtype: turn_duration` (once per completed turn) |
| `file-history-snapshot` | 2 | Bookkeeping for the `/` file-edit undo stack |
| `user` | 2 | One per submitted turn — `message.content` is the plain text |
| `attachment` | 2 | Paired with each `user` line in this run (paste metadata) |
| `ai-title` | 1 | The auto-generated session title |
| `assistant` | 4 | The model's replies — **two lines per turn**, same `message.id`, byte-identical `usage` on both. Only these carry usage. |

Only `assistant` lines matter for a gauge. Their shape:

```json
{
  "type": "assistant",
  "message": {
    "role": "assistant", "id": "…", "model": "deepseek-v4-flash",
    "content": […], "stop_reason": "…",
    "usage": {
      "input_tokens": 96,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 27648,
      "output_tokens": 2,
      "service_tier": "standard", "server_tool_use": {…}, "cache_creation": {…}
    }
  },
  "cwd": "…", "sessionId": "…", "timestamp": "…", "uuid": "…", …
}
```

**`usage` lives under `message.usage`, not at the top level.** No other line
type carries it — the three `system` lines, both `user` lines and both
`attachment` lines were checked and none has a `usage` key anywhere.

---

## 3. The field that matters, and the one that would have lied

Both turns' usage, side by side:

| Turn | `input_tokens` | `cache_creation_input_tokens` | `cache_read_input_tokens` | sum |
|---|---:|---:|---:|---:|
| 1 | 27,723 | 0 | 0 | **27,723** |
| 2 | 96 | 0 | 27,648 | **27,744** |

**`input_tokens` alone updates mid-session, but reading it alone would be a
faked number, not a measured one.** Turn 2's `input_tokens` is 96 — DeepSeek's
Anthropic-compatible endpoint cached turn 1's ~27.7k tokens and only billed
the new material as fresh input, moving the rest to
`cache_read_input_tokens`. A gauge that read `input_tokens` in isolation would
report worker-2 dropping from "≈14% of window" to "≈0%" the instant caching
kicked in — the opposite of what actually happened; the conversation only grew.

**The honest figure is the sum of all three input fields**
(`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`),
which is what the API actually charged as prompt tokens for that turn and
tracks monotonically with the conversation's real size (27,723 → 27,744,
consistent with one more short exchange). This is the number
`context_gauge::sample_gauge` reads.

**Confirms the spike's actual question:** yes, the usage fields update
mid-session — turn 2's numbers are not turn 1's, and the *sum* moves the
direction a growing conversation should.

**No `deepseek-v4-flash`-unrecognized notice appears in the transcript.**
The message WP-02 found (`"deepseek-v4-flash" is not a model this version of
Claude Code recognizes, so auto-compact will keep this session within 200k
tokens…`) is TUI/stderr output, not a line CC persists to the JSONL — none of
this session's three `system` lines mention it. That finding stands on its own
evidence (`system-prompt-notes.md` §4); this spike neither confirms nor
weakens it, and it is the reason the gauge's window denominator is **our own**
Tier 2 constant (`decisions.md`) rather than anything read from the transcript
— nothing in the file states what window CC or DeepSeek think applies.

---

## 4. What this settles for the implementation

- **Sample by scanning the directory at read time, never by caching a
  filename from spawn.** `/clear` is documented (`command-channel-notes.md`)
  to reset a pane's session; whether that also opens a *new* `<uuid>.jsonl` or
  reuses the current one was not tested here (it would be a fifth spike run
  for a question the implementation doesn't need answered either way) —
  `context_gauge::sample` picks the **most-recently-modified** `*.jsonl`
  under `projects/<slug>/` on every call, which is correct under both
  possibilities and costs nothing extra (one directory read).
- **Read from the end.** The last `assistant` line with a `usage` object in
  the file is the pane's current figure; earlier ones are stale by
  definition. Two identical lines per turn (§2) make "last" and "last two"
  equivalent, so no de-duplication is needed.
- **`used_tokens = input_tokens + cache_creation_input_tokens +
  cache_read_input_tokens`.** Anything reading `input_tokens` alone is
  building the exact "band metrics we don't yet track are omitted, not
  faked" failure decisions.md already warns against (L155) — a plausible
  looking number that is wrong in the misleading direction.
- **No transcript, or a transcript with zero `assistant` lines yet, is
  `None` — never zero, never a chars/4 guess.** A pane that has just spawned
  and not yet completed a turn has nothing measured about it; "0%" would
  read as "empty and safe" when the true answer is "unknown." The
  chars/4 fallback the requirements doc's design sketch floats is used only
  for the **spawn-time estimate** (over our own rendered brief text, which we
  hold in memory and never need a file read for) — not for the live gauge,
  which only ever reports a real usage-derived number or nothing.
- **The window denominator does not come from the transcript.** It is a
  Tier 2 constant recorded in `decisions.md`, deliberately not CC's own
  200k-for-an-unrecognized-model assumption (WP-02's finding, restated in the
  package brief for this session).

---

## 5. Reproducing it

```console
$ cd examples/context-gauge-spike
$ python3 probe.py --seconds 75
```

Everything under `work/cc-config-live/projects/` is the isolated worker's own
config dir — never the operator's `~/.claude`, which this spike does not read
and the shipped sampler must not either.

**Re-measure on a Claude Code update.** `building.md` §8 lists flag and
schema drift across versions as a live risk, and the transcript format above
is exactly the kind of internal detail a CC release could change without
announcing it. The claim most likely to move is the `usage` field set itself;
the path/slug rule is the same one `tui-spawn-notes.md` already depends on
for the config-dir trust key and has been stable across this project's prior
spikes.
