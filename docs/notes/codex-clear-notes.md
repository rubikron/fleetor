# codex context-clear notes

**Version stamp: `codex-cli 0.153.4`.** macOS 15.7.4, arm64, npm install. Stamped with the
vendor build id rather than a date, for `codex-spike-notes.md`'s reason: the claim has to
stay re-checkable rather than become folklore.

Re-runnable in one command, spending **zero tokens**:

```
python3 examples/codex-spike/probe_clear.py
```

A sibling of `probe.py`, not a ninth arm in it. `probe.py`'s eight arms and its exit-code
contract are gated by `src-tauri/tests/vendor_binary_tier.rs`, and this measurement needs a
capture server that numbers *every* request instead of overwriting one file, because two
turns have to be told apart. It reuses `probe.py`'s scratch installation wholesale — the
catalog clone, the fabricated provider, the sentinel brief — so the "missing field
`shell_type`" lesson stays in one place.

**Never rewritten** (`building.md` §4). Findings append.

---

## The question

C3 settled that `model_instructions_file` **replaces** the vendor's built-in system prompt:
with it set, the request's `instructions` field went from 17,730 characters of "You are
Codex, an agent based on GPT-5…" to the sentinel file, and "You are Codex" was absent from
the whole body. That is the replace-not-append property D-043 needs, and it is not reopened
here.

What C3 explicitly left unmeasured is what happens after `/clear`. `codex-spike-notes.md`
records it in those words: *"It is a file the harness reads rather than a value snapshotted
at boot, which is why it is expected to. Expected is not measured."*

The failure it guards against is this arc's signature one — a pane silently loses its
identity mid-run while looking perfectly healthy. A worker that has forgotten it is a
FLEETOR worker still renders a prompt, still accepts a paste, still answers.

## The answer

**Yes. The brief survives `/clear`, under both spellings of the carrier.**

The turn issued after a `/clear` carries the same 50-character brief in `instructions` that
the turn before it carried — byte-identical — and the vendor's built-in prompt does not come
back.

## Method

The instrument is C13's: a fabricated `[model_providers.probe]` pointed at
`http://127.0.0.1:8732`, answering every POST with `data: [DONE]`. That renders the literal
request body codex would have sent — `instructions`, `input`, `tools` — and never completes
an assistant turn, so the whole measurement costs nothing.

Driving `/clear` needs a real pty, so the probe forks one, runs the interactive TUI in it
with FLEETOR's own `TERM`/`COLORTERM`, and drives: one turn, `/clear`, another turn. Each
message is delivered as a bracketed paste followed by CR (M26's path); `/clear` is delivered
as one unframed write so its `/` reaches column 0 (D-045).

Two carriers are measured rather than one being reasoned from the other:

| carrier | spelling | who uses it |
|---|---|---|
| `config` | `model_instructions_file` in the seeded `CODEX_HOME/config.toml` | a FLEETOR pane (C6's `seed_config_dir`) |
| `flag` | `-c model_instructions_file=…` on the command line | `probe.py`'s arm 1 |

## The evidence

Four requests reach the capture server on the `config` carrier — one user turn and one
title-generation request per turn. `instructions` is quoted literally:

| request | `instructions` | `You are Codex` in body | pre-clear turn in `input` |
|---|---|---|---|
| 1 — the pre-clear turn | `'SENTINEL-MODELINSTR\nYou are a FLEETOR worker pane.'` (50 chars) | absent | present |
| 2 — its title request | same 50 chars | absent | present |
| 3 — **the post-clear turn** | **same 50 chars** | **absent** | **absent** |
| 4 — its title request | same 50 chars | absent | absent |

Request 3 is the whole finding. Its `input` roles are
`['developer', 'developer', 'developer', 'user', 'user']` — codex's own skills/multi-agent
instructions, then `AGENTS.md`, then the environment context, then the post-clear message —
and the pre-clear message is gone from it while the brief is not.

### The control, and why it is an arm rather than a remark

`clear-actually-cleared` asserts that the post-clear request's `input` no longer carries the
pre-clear sentinel. Without it the measurement is vacuous: if `/clear` had silently failed,
the second turn would be an ordinary continuation of the first, and an unchanged
`instructions` field would prove nothing whatsoever. This was not hypothetical — three
earlier drafts of the probe produced exactly that reading, a post-clear turn still carrying
the pre-clear message, and each time the cause was the pane never having received the
`/clear` at all.

### The negative control

The probe was run once with the carrier deliberately removed. `instructions` came back as
**17,730** characters and `You are Codex` was present, and the arms went red:

```
FAIL  clear-brief-before  — instructions=17730 chars
FAIL  clear-brief-survives  — instructions=17730 chars, 'You are Codex'
```

That is the shape a "no" would have had, and it independently reproduces C3's 17,730 in the
**interactive TUI** — C3 measured it through `codex exec`.

## The five arms

| arm | what it asserts |
|---|---|
| `clear-brief-before` | the pre-clear turn carries the brief, not the built-in prompt — C3's property, re-measured in the interactive TUI |
| `clear-actually-cleared` | the post-clear turn's `input` no longer carries the pre-clear sentinel |
| `clear-brief-survives` | **the question** — the post-clear turn's `instructions` still carries the brief, and `You are Codex` is absent |
| `clear-brief-identical` | `instructions` is byte-identical either side of the clear, ruling out a partial or truncated re-read |
| `clear-brief-survives-via-flag` | the question again with the brief carried on the command line |

All five pass on `codex-cli 0.153.4`, in about 40 seconds.

---

## Findings that were not the question

**The load-bearing "startup wait" C22 recorded is codex waiting out unanswered terminal
queries.** At startup codex emits `CSI 6n` (cursor position), `OSC 10;?` and `OSC 11;?`
(foreground and background colour), `CSI ?u` (kitty keyboard) and `CSI c` (device
attributes), and paints nothing usable until they are answered. A real terminal answers
them; a bare `pty.fork()` never does, so codex waits them out. Answering the five queries
from the probe — and setting a window size, because `pty.fork()` leaves the terminal 0x0
and codex renders empty frames into a 0x0 terminal — is what let the fixed sleeps go away.

**This does not settle C22's table, and it is worth being exact about that.** Three things
changed together before this probe stopped needing a startup sleep — the window size, the
five query answers, and the splash keypress — and which of them explains why `probe.py`'s
6 s wait failed and its 12 s wait worked was **not** isolated. C22's numbers stand as
recorded.

What *is* settled is narrower and still worth having: a startup sleep here was standing in
for conditions that can be **detected**, and once they are detected the sleep is not needed
at all. That is the case C26 makes for refusing a `startup_wait` field, arrived at from a
second direction.

**A fresh `CODEX_HOME` opens on an animated splash that ends on a keypress, not on a
timer.** Left alone it was still animating after 75 seconds. This is what swallowed the
first turn in three earlier drafts. `Enter` on an empty composer submits nothing, so the
probe presses it until the pane goes quiet — a readiness signal rather than a guessed sleep.

The composer's placeholder is **not** a usable readiness signal: it paints once and is then
scribbled over by the splash, so waiting for it fires while the pane is still ignoring
input. What works is the conjunction of the model slug appearing in the status line (it
reads `model:loading` until the config resolves) and the pane having stopped repainting.

**`/clear` is not sensitive to how it is typed.** One raw 6-byte write and byte-at-a-time
delivery at 80 ms both work once the pane is awake. An earlier draft of this note claimed a
raw burst was dropped; that was wrong, and the claim was withdrawn after measuring it —
the drops were the splash, not the burst.

**Retries are provider-level.** `request_max_retries` and `stream_max_retries` set inside
`[model_providers.*]` cut each turn from a dozen identical requests to one. Set at the top
level they do nothing. This is why the probe runs in 40 seconds rather than four minutes.

**`AGENTS.md` is re-injected after the clear**, as `role: "user"`, exactly as C3 recorded it
arriving before the clear. So the in-band cost C3 names is paid again on every clear, not
once per pane.

---

## Not measured

**Whether the brief file is re-read per turn or snapshotted at pane start.** Nothing here
distinguishes them: the file was not mutated mid-session, so both hypotheses predict the
byte-identical `instructions` that was observed. It matters only if FLEETOR ever wants to
change a running pane's brief without respawning it, which nothing currently does.

**Whether `/compact` behaves like `/clear`.** C10 lists both on the command channel. Only
`/clear` was driven.

**This file is not yet wired into the vendor-binary tier.** `vendor_binary_tier.rs` pins
`probe.py` by name and gates on its exit code; it does not run `probe_clear.py`. Wiring it
in is a Rust change and was out of scope for this spike. Until it happens this probe is
runnable but not gated, so a regression in it would be silent — which is the exact failure
mode C13's skip discipline exists to refuse, and it should be closed by whoever next touches
that tier.
