# `--system-prompt` — what a pane loses, and what it keeps

Measured against **Claude Code 2.1.223** on macOS 24.6.0, via
`examples/system-prompt-spike/probe.py`. The sibling of
[`tui-spawn-notes.md`](./tui-spawn-notes.md), which bisected the config seed;
same rule applies — **everything below is verified by running it**, and where
this disagrees with a plan, this wins (`building.md` §4).

The question WP-02 had to answer before switching a flag: FLEETOR's panes were
briefed with `--append-system-prompt`, so our brief sat *after* Claude Code's
own system prompt and could not override it. `--system-prompt` **replaces** that
prompt. What exactly goes away, and does an interactive pane still work?

---

## 0. How this was measured

Two harnesses, because the questions split cleanly in two.

**Record mode — what CC assembled.** `ANTHROPIC_BASE_URL` points at a local
Anthropic-compatible server that writes every request body to disk and streams
back a canned reply. The system prompt is then a *file*, not an inference from
how a model behaved. Two runs — one `--system-prompt`, one
`--append-system-prompt`, identical in every other respect — and the diff is the
answer. Costs nothing, and is deterministic in a way "ask the model what it can
see" never is.

**Live mode — whether the pane works.** The real worker posture from
`src-tauri/src/spawn.rs`: DeepSeek Flash, isolated seeded `CLAUDE_CONFIG_DIR`,
`ANTHROPIC_API_KEY` removed, `--permission-mode auto`. For the things a fake
endpoint cannot answer — does it reach a prompt, does it run tools, do the
slash commands still work.

The fixture gives every context source its own nonsense marker (project
`CLAUDE.md`, user `CLAUDE.md`, a skill, the brief) so a recorded request says
exactly which of them survived, rather than which of them *probably* survived.

---

## 1. The headline: one block shrinks, nothing else moves

The recorded request under each flag, field by field:

| Field | `--append-system-prompt` | `--system-prompt` |
|---|---|---|
| `system[0]` — billing header | 70 chars | **identical** |
| `system[1]` — "You are Claude Code, Anthropic's official CLI for Claude." | 57 chars | **identical** |
| `system[2]` — CC's guidance + our brief + git status | **6,866 chars** | **364 chars** |
| `tools` | 28 definitions | **identical** |
| first user message (`<system-reminder>`) | 18,594 chars | **identical** |
| agent-types system block | 7,015 chars | **identical** |

**`--system-prompt` changes exactly one thing.** It empties CC's guidance out of
the third system block and leaves our brief plus the git-status section in it.
Tools, memory files, skills, agents and the whole first-user-message context
block are untouched — byte-for-byte, across both runs.

That is the finding that made the switch safe, and it is not what the flag's
name suggests. "Replace the system prompt" turns out to mean "replace CC's
*guidance*", not "replace everything CC injects".

---

## 2. What actually vanishes

The 6.5 KB that is gone, section by section, from the recorded `system-append.txt`:

| Section | What it said | Does a pane need it? |
|---|---|---|
| (unheaded opener) | "You are an interactive agent that helps users with software engineering tasks." | Restated — it is the job. |
| (unheaded) | The security-testing posture: assist with authorized/defensive/CTF work, refuse destructive, DoS, mass-targeting, supply-chain and evasion. | **Restated.** Dropping refusal behaviour is not a parity change. |
| `# Harness` | Output is markdown in a terminal · a denied tool call means declined, don't retry verbatim · mid-conversation system turns and hooks are system-controlled · prefer dedicated file/search tools over shell · independent calls can run in parallel · cite `file_path:line_number`. | **Restated.** Flash workers degrade visibly without the tool-discipline lines. |
| (unheaded) | "Write code that reads like the surrounding code." | Restated in one clause. |
| (unheaded) | Pronoun guidance — they/them unless stated, never infer from a name. | **Restated.** It governs user-visible text. |
| (unheaded) | Confirm before hard-to-reverse or outward-facing actions; report outcomes faithfully. | **Restated.** A fleet where orch believes worker reports cannot afford to lose "if tests fail, say so". |
| `# Session-specific guidance` | The `! <command>` prefix · invoke `/<skill-name>` through the Skill tool. | Restated **for orch only** — a worker has no human at its keyboard and no skills in its isolated config dir. |
| `# Memory` | The whole persistent file-based memory protocol — where the directory is, the frontmatter schema, `MEMORY.md` indexing. | **Deliberately not restated** — see §5. |
| `# Environment` | cwd · is-git-repo · platform · shell · OS version · model name · current Claude model IDs · CC's other surfaces · `/fast`. | **cwd restated** (§3). The rest is either wrong for a pane or irrelevant to it. |
| `# Context management` | The conversation gets summarized and handed back — don't wrap up early or hand off mid-task. | **Restated.** A worker that thinks it must wrap up early abandons work mid-task. |

## 3. What survives — including two surprises

**The git-status section stays.** `--exclude-dynamic-system-prompt-sections`
says it is "ignored with `--system-prompt`", which reads like the dynamic
sections are skipped. They are not skipped — that *flag* is what gets ignored,
and the git block is still emitted:

```
gitStatus: This is the git status at the start of the conversation. …
Current branch: fleet/worker-1
Main branch (you will usually use this for PRs): main
Git user: mihitp
Status:
?? dirty.txt
Recent commits:
3f355d5 fixture
```

So a worker is still told its branch. WP-02's design sketch expected to restate
that; the spike says don't bother.

**`cwd` does not survive.** The `# Environment` section carried it and is gone,
and nothing else in the request names the working directory except — by
accident — the *paths* of any `CLAUDE.md` files that happened to load. A worker
in a worktree with no `CLAUDE.md` would not know where it is. This is the one
concrete loss the brief has to cover, and it is why `{cwd}` is now a required
placeholder in both templates.

**Memory files and skills load normally.** The first user message carries the
same `<system-reminder>` in both runs: project `CLAUDE.md`, user `CLAUDE.md`
from `CLAUDE_CONFIG_DIR`, and the skill listing. All four fixture markers found,
identical text. **The answer to "does the target repo's `CLAUDE.md` still load
for orch" is yes, unchanged** — it never travelled in the system prompt to
begin with.

**"You are Claude Code, Anthropic's official CLI for Claude." survives.** It is
its own system block and `--system-prompt` does not touch it. A brief telling a
pane "you are not Claude Code" would therefore contradict the block above it.

---

## 4. Does the pane still work? Yes, on every axis tested

| Question | Answer | Evidence |
|---|---|---|
| Does an interactive pane under `--system-prompt` reach its prompt? | **Yes** | Live DeepSeek run: mode line drawn, `❯` input box, no onboarding markers. |
| Does it read the brief? | **Yes** | Print-mode run with a replacement prompt saying "reply exactly PARITY-OK" answered `PARITY-OK`, exit 0. |
| Does it run tools normally? | **Yes** | Live run asked for a file's contents; the pane read it and answered `committed`, correctly. |
| Does `--permission-mode auto` still hold? | **Yes** | "⏵⏵ auto mode on" on the mode line, zero permission markers across all six runs. |
| Does `/clear` work? | **Yes** | Submitted `/clear`; the TUI reset to the CC banner and an empty prompt. |
| Does `/compact` work? | **Yes** | Submitted `/compact`; it ran and answered with its own "Not enough messages to compact." |
| Is the flag accepted on the operator's own auth path? | **Yes** | The print-mode run above used the operator's default credentials and model. |

**No fallback ladder was needed.** WP-02's guardrail said that if
`--system-prompt` degraded interactive behaviour, panes should split by class —
workers replace, orch appends. Nothing degraded, so all five panes replace, and
the per-pane-class split stays unbuilt.

### Two incidental findings, recorded because they cost time

**Slash commands must be pasted, not typed, from outside.** Typing `/compact`
one character at a time into the pty opened CC's command menu, and the
characters after `/` never reached its filter — `Enter` then selected the menu's
*first* entry (`/add-dir`) instead. Two runs were lost to this before switching
to a bracketed paste of the whole line, which filters and selects correctly.
This is the "injected bytes become menu navigation at a `/` prompt" risk from
`building.md` §8 observed directly, and it is a note for WP-03: a command
channel that types rather than pastes will fire the wrong command.

**CC does not recognise `deepseek-v4-flash`**, and says so:

> "deepseek-v4-flash" is not a model this version of Claude Code recognizes, so
> auto-compact will keep this session within 200k tokens (the context window it
> assumes). If the model accepts more, append `[1m]` to the model name for 1M,
> or set `CLAUDE_CODE_MAX_CONTEXT_TOKENS` to its real window.

Harmless today and out of WP-02's scope, but it means every worker's auto-compact
threshold is a guess. That belongs to WP-09's budget work.

---

## 5. The one thing deliberately not restated

`# Memory` — CC's persistent file-based memory protocol — is dropped and stays
dropped.

For a **worker**, the directory it points at lives under that slot's isolated
`CLAUDE_CONFIG_DIR`. Memories written there are invisible to every other pane
and to the operator, and `building.md` Tier 1.8 says shared knowledge merges
only after review. A per-slot memory nobody reviews and nobody reads is not a
feature, it is a place for a worker to talk to itself.

For **orch**, the memory directory is the operator's own. The fleet has no
business teaching their daily-driver `claude` a memory protocol it would
otherwise have had — but equally none removing it. This is the honest statement
of the tradeoff: replacing the system prompt takes it away, and WP-02 chose not
to put it back rather than to reimplement it half-way.

**What would reverse this:** an operator noticing their orch pane stopped
remembering things across sessions. The fix is one fragment in `prompts/`, and
`scaffolding.md` is where it would go.

---

## 6. Reproducing any of it

```console
$ cd examples/system-prompt-spike
$ python3 probe.py --mode record --flag system --tag sysprompt --send "hello" --seconds 22
$ python3 probe.py --mode record --flag append --tag append   --send "hello" --seconds 22
$ diff work/system-append.txt work/system-sysprompt.txt

$ python3 probe.py --mode live --flag system --send "run: cat already.txt" --seconds 70
$ python3 count.py            # the WP-09 budget baseline
```

Run outputs land in `work/` and are gitignored. Live runs bill DeepSeek Flash;
the six runs behind this document cost well under a cent.

**Re-measure on a CC update.** This document is version-stamped for the same
reason `tui-spawn-notes.md` is: `building.md` §8 lists "CC's flags or config
keys drift across versions" as a live risk, and the whole of §1 above is a
statement about one build's behaviour.
