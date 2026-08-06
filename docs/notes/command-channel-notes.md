# Slash commands from outside the pane — what a pasted `/clear` and `/compact` do

Measured against **Claude Code 2.1.223** on macOS 24.6.0, via
`examples/command-channel-spike/probe.py`, six live runs on `deepseek-v4-flash`.
The third of the version-stamped measurement docs, after
[`tui-spawn-notes.md`](./tui-spawn-notes.md) (which config keys a pane needs) and
[`system-prompt-notes.md`](./system-prompt-notes.md) (what `--system-prompt`
takes away). Same rule: **everything below is verified by running it**, and where
this disagrees with a plan, this wins (`building.md` §4).

The question WP-03 had to answer before adding a verb: `fleet send` frames every
message as `[fleet · orch] …` and delivers it as a bracketed paste, so a body
beginning with `/` arrives as *text about a command*. A command channel needs the
`/` to be the first character in the input box. Does that actually execute, and
what happens when the box is not empty or the pane is busy?

---

## 0. What was already known, and what it left open

WP-02's spike found — accidentally, and at the cost of two runs — that **typing** a
slash command one character at a time does not work: the `/` opens Claude Code's
command menu, the characters after it never reach the menu's filter, and `Enter`
selects the menu's *first* entry (`/add-dir`). It also found that a **bracketed
paste** of the whole line filters and selects correctly.

So the mechanism was settled before this spike started, and WP-03's own plan —
which specified "raw bytes, no bracketed paste" — was already disproven. This
spike does not re-ask it. It asks the three things the delivery arm depends on,
all about a *pasted* command:

1. Empty input box — does it run? With arguments?
2. Text already sitting unsubmitted in the box — what does the paste become?
3. Mid-turn — does it execute, queue, or land as prose?

Every probe writes exactly the bytes `PaneRegistry::write_paste` writes:
`\x1b[200~` · the command · `\x1b[201~` · **30 ms** (`SUBMIT_GAP`) · `\r`. No
timing variant was needed — see §4.

---

## 1. The headline

| Situation | `/compact <args>` | `/clear` | Fires? |
|---|---|---|---|
| Empty input box | `⎿ Not enough messages to compact.` | banner redrawn, fresh session | **yes** |
| Unsubmitted text in the box | submitted as prose | submitted as prose | **no** |
| Mid-turn (tool call in flight) | queued, then `⎿ Compacted` | queued, then context gone | **yes, after the turn** |

Two of those are the reason `accepted` may never be rendered as "executed"
(Tier 1.5). The middle row is a real, reachable failure that nothing on this side
of the pty can detect.

---

## 2. Empty input box — both run, arguments included

`probe.py --scenario empty`. Pasted `/compact focus on the current task`, then
later `/clear`, then a plain message to prove the pane still worked.

```
❯ /compact focus on the current task
  ⎿  Not enough messages to compact.
```

`/compact` **took its arguments** and executed; the "not enough messages" refusal
is Claude Code's own, which is the proof it ran as a command rather than being
read as text. `/clear` redrew the startup banner and the following send answered
normally, so the session had really been reset and the pane was still usable.

**The two commands reach the same place by different routes**, and the difference
is worth knowing:

- `/compact <args>` — the space and the argument text close the command menu, and
  the line executes directly. No menu is involved.
- `/clear` — with no arguments the command menu stays open, *filtered by the whole
  pasted string*, with `/clear` as its first entry. `\r` selects that entry.

```
❯ /clear
/clear          Start a new session with empty context; previous session stays on disk…
/code-review    Review the current diff…
/simplify       Review the changed code…
```

That filtered list is exactly what WP-02 could not get by typing, and it is why
pasting works: the filter sees the whole string in one input event. The
`/add-dir` marker — the wrong entry WP-02's typed run selected — **never appeared
in any of the six runs**.

**The residual risk, stated plainly:** an argument-less command is selected by
ranking, not by exact match. `/clear` ranks first for the string `/clear` on
2.1.223. If a future build's fuzzy ranking put something else first, `\r` would
fire that instead, and nothing on our side would know. `/compact` is not exposed
to this when it carries arguments — which is one more reason the worker brief
teaches `/compact <what to keep>` rather than a bare `/compact`.

---

## 3. Text already in the box — the command becomes prose

`probe.py --scenario queued`. Text was pasted **without** a submitting `\r`, the
way an operator's half-typed line sits in the input box, and then the command was
pasted normally.

```
❯ half a sentence the operator was still writing/compact keep the current task
```
```
❯ another unfinished thought/clear
⏺ Understood — clearing the unfinished thought. What would you like me to do?
```

The paste is **appended at the cursor**. The `/` is no longer the first character,
so nothing is a command: one concatenated line is submitted as an ordinary
message and the model answers it conversationally. Both commands, same result.

This is the failure the whole `accepted` vocabulary exists for. The bytes reached
a live pty — that part is true and is all `accepted` has ever claimed. Whether
the command *fired* is not observable from outside the TUI, and WP-03 does not
claim it.

**A mitigation exists and was deliberately not taken.** Prefixing the paste with
a line-kill (`\x15`) or an `ESC` would empty the box first and make the command
land at column 0 every time. It is not in this package because on `orch` — the
one pane with a human at its keyboard — it silently destroys whatever the
operator was typing, and choosing that trade is not a builder's call. Recorded
here so the option is findable, with its cost attached, if a live shakedown shows
the concatenation happening in practice.

---

## 4. Mid-turn — Claude Code queues it, then runs it

`probe.py --scenario busy-compact` / `busy-clear`. A `sleep 45` under
`--permission-mode auto` is the only reliable way to hold a Flash pane busy for a
known number of seconds; `--scenario midturn` is kept in the script as the
negative result that proves it, because `deepseek-v4-flash` answered a
count-to-forty in three seconds and the "mid-turn" command landed at an idle
prompt.

While the tool call was in flight, the submitted command visibly became a
**queued** input rather than a discarded one:

```
⏺ Bash(sleep 45; echo SLEPT)
  ⎿  Running… (15s · timeout 1m)
❯ /clear
❯ Press up to edit queued messages
```

and when the turn ended it was released **as a command**:

```
⏺ SLEPT
✻ Worked for 48s
❯ /compact keep the sleeping task
  ⎿  Compacted (ctrl+o to see full summary)
```

For `/compact` that transcript is conclusive. For `/clear` it is not — the
command menu repaints the scrollback, banner and all, so a redrawn banner cannot
be told apart from a real reset by reading the byte stream. So the sixth run asks
the pane instead (`--scenario clear-witness`): plant a codeword, start a long
turn, paste `/clear` mid-turn, and after the turn ask for the codeword back.

```
❯ what is the codeword? if you do not know one, say NOCODEWORD.
⏺ NOCODEWORD
```

The queued `/clear` had executed. **A mid-turn command is deferred by Claude Code,
not lost, and not degraded into text.**

**No timing change was needed anywhere.** All six runs used `write_paste`'s exact
30 ms gap between the closing paste marker and the `\r`. WP-03's plan asked
whether the menu popup needs a longer gap; it does not, and the delivery path's
one and only delay (D-034) stays as it is.

---

## 5. What this settles for the implementation

- **The command channel is a caller of `write_paste`, not a new byte path.** The
  measured working sequence *is* `write_paste`'s sequence. What differs is only
  the body: unframed, so `/` sits at column 0. `Message::framed`, `sanitize`,
  `write_paste` and `Op::Send`/`Broadcast`/`Reply` are untouched by WP-03.
- **A command must never share a write with a message.** The per-pane writer
  drains concurrent messages and joins them with a blank line (D-039); a command
  inside such a join would not start at column 0 and §3 is what it would become.
  So consecutive messages batch exactly as before, and a command is a write of
  its own.
- **`accepted` means bytes reached a live pty. Nothing more.** §3 and §4 are two
  different reasons the same `accepted` can precede a command that never ran and
  a command that ran a minute later.
- **Teach `/compact <what to keep>`, not bare `/compact`.** Arguments bypass the
  menu-ranking risk in §2, and they are better compaction anyway.

Left open on purpose: `--autocompact <auto|tokens>`, noticed on 2.1.223, is a
spawn-time Tier 2 lever that could complement a deliberate `/compact`. Untouched
this session — it belongs with WP-09's budget measurements.

---

## 6. Reproducing any of it

```console
$ cd examples/command-channel-spike
$ python3 probe.py --scenario empty          # §2
$ python3 probe.py --scenario queued         # §3
$ python3 probe.py --scenario busy-compact   # §4
$ python3 probe.py --scenario busy-clear     # §4
$ python3 probe.py --scenario clear-witness  # §4, the decisive one
$ python3 probe.py --scenario midturn        # the negative result behind busy-*
```

Run outputs land in `work/` and are gitignored. Live runs bill DeepSeek Flash;
the six runs behind this document cost well under a cent.

**Re-measure on a Claude Code update.** Every statement above is about one
build's input handling, and `building.md` §8 lists flag and behaviour drift
across versions as a live risk. The three claims most likely to move are the
menu-ranking of a bare `/clear` (§2), the concatenation behaviour (§3), and the
mid-turn queueing (§4).
