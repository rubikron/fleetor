# Critiquing a run with no answer key

Measured 2026-09-02 · archives written by **Claude Code 2.1.224** (orch on `claude-opus-5`
and `claude-opus-4-6`, workers on `deepseek-v4-flash`) · critiques run under **CC 2.1.258**,
Sonnet · reproduce by pasting the prompt in §6 into a session pointed at any run directory
under `~/.fleetor/runs/`.

Zero product code. This is the D15 spike, required by `building.md` §4 before WP-23 ticket 08
is built. The unknown is the one D10 names: **a judge with no answer key is warned to converge
on "solid work, maybe more tests."**

---

## The verdict

**It is worth reading, and ticket 08 should be built.** The narrowed brief produced findings
that are specific, checkable and about the fleet rather than the code — including two that
change what an operator would do next: a run whose reviewed work was never integrated because
the integration worktree belonged to *a different repository*, and a run where `orch`
abandoned its own workers and edited the target in place, uncommitted, until the operator
interrupted it. Neither is an opinion. Both carry a timestamp, a file and a line.

The failure mode D10 warns about **did not appear under the narrowed brief.** It did appear —
in a more dangerous form than predicted — under an unconstrained control prompt run against
the same archive. See §2, because that result is the reason the brief is shaped the way it is.

Two of the arc's five named categories produced **nothing on either run**, and a sixth
category added during the spike produced the most valuable findings on both. That is the
material change this note recommends to ticket 08's brief.

---

## 1. What was critiqued, and why those

42 archives exist under `~/.fleetor/runs/`. Most are `no activity` — a start-then-quit with
nothing but boot notices. Two are substantial enough to critique, and they conveniently form
the pair the ticket asks for.

| | `2026-08-08T00-32-58Z` — **run A** | `2026-08-08T01-13-20Z` — **run B** |
|---|---|---|
| Target | `~/.fleetor/eval-target` (a 25-line `loglevel.py`) | `~/.fleetor-eval/workspaces/pyflakes-global-in-class/repo` |
| Events / messages | 48 / 26 | 39 / 12 |
| Blocks posted | 1 | 3 |
| Blocks that reached a check | 1 | **0** |
| Transcripts | 5 | 5 |
| Judged | **went well** | **went badly** |

**How I judged which was which** — before running any critique, and from the event log alone.
Run A's single block was posted with criteria, claimed, checked, peer-reviewed by a second
worker who ran a five-mutation kill test, and approved. Run B posted three blocks naming
`pyflakes/checker.py` and `pyflakes/test/test_other.py`, files that existed in **no worker
worktree** — every worktree held the unrelated `loglevel` project — and all three blocks died
without a check.

That judgement is deliberately recorded here *before* the critiques, because the interesting
question is whether the Critic reaches the same conclusion from the same evidence. It did, on
both runs, and on run A it found the run was worse than my own reading had it.

**Method.** I built independent ground truth for both runs first — every task event, every
message with sender and recipient, every file-writing tool call across all ten transcripts —
so that each claim a critique made could be scored true, false or uncitable rather than taken
on faith. Every finding reproduced below was re-derived from the archive before being written
down. Two claims I could not verify from my own extraction (`git checkout -b` in run B, and
`orch`'s closing message in run A) were checked by reading the transcript records directly.

Nothing under `~/.fleetor/` was written, moved or modified at any point.

---

## 2. The control: the failure mode is real, but it is not the one D10 named

A deliberately unconstrained prompt — *"Read it and critique this run. Tell me how it went and
what should be improved."* — was run against run A as a control.

It did **not** produce "solid work, maybe more tests." It produced something fluent,
substantive and largely **true** — it independently found the stale integration worktree — and
it was **unusable**, for three reasons:

1. **Not one citation.** No `seq`, no line number, no timestamp anywhere in the output. Every
   claim, including the correct and important ones, was unanchored. An operator could not
   check a single sentence without redoing the whole analysis.
2. **Constant remit breach.** "worker-2's review wasn't a rubber stamp", "was sharp", "the
   better instinct" — judgements of the *work*, which D10 forbids because the Critic has no
   grounds for them.
3. **Prescription.** "should be namespaced per-target", "`/dev/null` should be allowlisted
   globally" — design decisions presented as conclusions, which the arc places outside the
   Critic's remit entirely.

**This is the load-bearing result of the spike.** The risk is not that an answer-key-less judge
produces bland mush; it is that it produces *confident, accurate-sounding, uncheckable
prose that quietly grades the work*. Mush would be obvious and harmless. This is neither.
Every constraint in §6 exists to close one of these three holes, and the narrowed runs closed
all three: **no output under v2 or v3 contained a single "should", a single item of praise, or
a single uncited finding.**

---

## 3. Findings — run A (`2026-08-08T00-32-58Z`)

Produced by the §6 prompt. Citations verified against the archive.

**A block was marked `done` 2.8 seconds before its own check was invoked.**
`fleet task update task-1786149656653-1 --status done` ran at 00:42:44.306Z
(`transcripts/worker-1/fd965a8d….jsonl` line 75), landing on the board at 00:42:45.503Z
(`events.json` seq 20, line 181). The check that gates that status —
`fleet done task-1786149656653-1 "python3 -m pytest tests/test_loglevel.py -q"` — was not
invoked until 00:42:47.095Z (same transcript, line 79), and its receipt reported
`73d30aa + uncommitted changes` (`events.json` seq 21, line 191). worker-1 reverted its own
status to `claimed` 1m54s later with the note *"correcting my earlier premature done"*
(`events.json` seq 29, line 277).

**The reviewed work was never integrated, because the integration worktree belonged to a
different repository.** After worker-2's APPROVE, `orch` ran
`git -C ~/.fleetor/_shell/worktrees/integration merge --no-ff fleet/worker-1` at 00:46:34.896Z
(`transcripts/orch/c74a6c62….jsonl` line 185) and got *"Already up to date."* (line 187). It
verified rather than trusting that — *"'Already up to date' is not what I expected"* (line 189)
— and found the worktree's git-common-dir was `/Users/bubblyducks/.fleetor/testbed/.git`, not
the run's own `eval-target/.git` (line 203, 00:46:58.863Z), its reflog already carrying merges
from unrelated history. `orch`'s closing message (line 210, 00:47:19.360Z): *"nothing merged,
nothing damaged, and `fleet/integration` for this repo still doesn't exist."* **The run's only
reviewed work ends on `fleet/worker-1` and nowhere else.**

**Two of four workers were idle for the entire run.** worker-3 and worker-4 have no transcript
record at all between spawn at 00:35:33.164Z / .171Z (`events.json` seq 7 line 46, seq 8 line
53) and a broadcast reaching them at 00:44:59.609Z / .644Z (seq 33 line 320, seq 34 line 331)
— **9m26s each**, of a 14.5-minute run. Neither was ever assigned a block.

**The same unverified evidence string was recorded as a handoff twice.**
`handoff-1786149507901-0` at 00:38:27.901Z (`events.json` seq 9, line 65) and
`handoff-1786150030124-29` at 00:47:10.124Z (seq 47, line 462) both carry
`--evidence "operator confirmed the app compiled and started"`. Each is immediately followed by
a notice that the workspace *"has no ground truth to be judged against"* (seq 10, seq 48).
`orch` recorded it while stating it could not verify it: *"That claim is yours, not mine"*
(`transcripts/orch/…` line 32).

**The final review verdict got no reply.** worker-2's `REVIEW VERDICT: APPROVE` at 00:46:20.269Z
(`events.json` seq 45, line 438) drew no answer. `orch`'s transcript shows it read the review
and went straight to the merge attempt (line 184). The run ended 50s later.

**One write-guardrail rule was independently rediscovered by three panes.** A Bash redirect to
`/dev/null` was refused four times: worker-2 at `events.json` seq 14 (line 121) and seq 16
(line 141), worker-1 at seq 17 (line 148), worker-4 at seq 40 (line 388). Nothing in the log
shows one pane's encounter changing another's subsequent commands.

---

## 4. Findings — run B (`2026-08-08T01-13-20Z`)

**`orch` abandoned the fleet and did the work itself, uncommitted.** After broadcasting
*"Stand down — … I'm implementing the fix directly"* to all four workers at 01:19:48–49
(`events.json` seq 32–35), `orch` ran `git checkout -b fleet/fix-global-in-class-body`
(`transcripts/orch/bd4cd109….jsonl` line 196, 01:19:53.265Z) **in the live target repository**,
then made four `Edit` calls to `pyflakes/checker.py` (lines 204, 212, 221, 228) and one to
`pyflakes/test/test_other.py` (line 259). **No `git add` and no `git commit` appears anywhere
in its 271-line transcript** — independently confirmed: zero matches.

**The run ends on an operator interrupt, mid-verification.** `orch`'s attempt to run the tests
it had just written (line 268, 01:21:09.546Z) was rejected — *"The user doesn't want to proceed
with this tool use… STOP what you are doing"* (line 269) — followed by
`[Request interrupted by user for tool use]` (line 270, 01:21:10.489Z). That is the last record
in the archive. No commit, no test result, no block closure follows.

**Three blocks posted, zero reached a check.** No block ever reaches `"status": "done"`, and no
`fleet done` appears in any of the five transcripts. Two were `dropped`; the third —
task-1786151857053-2, worker-3's reviewer block (`events.json` seq 11, line 117) — was **never
claimed, never dropped and never closed.**

**worker-4 was assigned nothing at all.** Its only inbound event in the entire run was the
stand-down broadcast at 01:19:49.019Z (`events.json` seq 35, line 330) — 6m27s after spawn at
01:13:22.504Z (seq 8, line 53). Its transcript is 11 lines long.

**An escalation to the operator was never answered.** worker-2 → `operator` at 01:19:04.599Z
(`events.json` seq 29, line 267) carries `"accepted": false`. `"operator"` never appears as a
`from` anywhere in the log's 39 entries.

**An assignment promised a follow-up that never came.** `orch`'s block message to worker-3
(`events.json` seq 18, line 181) says *"I will tell you when both are finished."* No such
message appears in the remaining 21 entries. worker-3 never sent a message at all.

**A worker read a file while `orch` was mid-edit on it.** worker-3 read the live
`pyflakes/checker.py` at 01:20:18.794Z (`transcripts/worker-3/0e6ebe5c….jsonl` line 143) —
between `orch`'s third and fourth edits to that same absolute path — and recorded the
contradiction it saw: *"the real repo's `ClassScope` at line 532 already shows
`self.indirect_assignments = {}`… That contradicts the bug being present."*

**Three workers each separately rediscovered the same provisioning fault** through wide
`find /` searches: worker-1 at 01:18:31.095Z, worker-2 at 01:18:55.660Z, worker-3 at
01:19:55.669Z. One such search took nearly 20 seconds to return.

---

## 5. Per-category results

Categories 1–5 are the arc's (D10, story 31). Category 6 was **added during this spike**; see
below.

| # | Category | Run A | Run B |
|---|---|---|---|
| 1 | Idle time | **✅ 2 findings** — worker-3 and worker-4 idle 9m26s each, the whole run | **✅ 4 findings** — worker-4 assigned nothing at all |
| 2 | Blocks marked done whose check never ran | **✅ 1 finding** — `done` 2.8 s *before* the check; never durably `done` again | **— nothing.** No block ever reached `done` |
| 3 | Blocks posted with no performance criteria | **— nothing.** The one block carried `semantic` + `technical` | **— nothing.** All three carried `semantic` + `technical` |
| 4 | Two workers editing one file | **— nothing.** Separate worktrees; correctly ruled out | **— nothing.** Only `orch` wrote |
| 5 | Messages that got no reply | **✅ 1 finding** — the final APPROVE verdict | **✅ 2 findings** — incl. an unanswered escalation to `operator` |
| 6 | *(added)* Anything else that cost the operator | **✅ 3 findings** — stale integration worktree; doubled unverified handoff; guardrail ×4 | **✅ 4 findings** — `orch` working uncommitted; the interrupt; a concurrent read of a live edit; three separate rediscoveries |

**Categories 3 and 4 produced nothing on either run**, and the reasons differ in kind:

- **Category 3 may be permanently quiet.** `fleet task post` appears to make `--semantic` and
  `--technical` easy enough that `orch` supplied them on all four blocks across both runs — in
  run B it supplied full criteria for work that was impossible to perform. The category cannot
  fire while the tool encourages the field. It is cheap to keep, and it will earn its place the
  first time a block is posted bare.
- **Category 4 is quiet because the design prevents it.** Every worker writes only inside its
  own worktree. Both runs contained a *tempting* false positive — run A's worker-2 edited
  `loglevel.py` five times during mutation testing — and the brief's trap 1 correctly ruled it
  out both times. **A category that reliably produces a true negative against a tempting false
  positive is doing work**, and it is the check that would fire the day worktree isolation
  breaks.

**Category 6 is the recommendation this spike makes to ticket 08.** The single largest defect in
each run — run A's uncompleted integration, run B's orchestrator going solo — falls outside all
five named categories. Without a bounded sixth slot the Critic reports a tidy run and misses
what actually cost the money. It was added with the same citation discipline and an explicit ban
on inference and prescription, and it did not reopen the mush door: every category-6 finding
above names a moment and cites it.

---

## 6. The winning prompt

Three versions were run. **v1** (five categories, citations required) already beat the control
decisively but drifted ~5 lines on `events.json` citations, listed six transcript lines for five
edits, paraphrased a pane's motive, and wrongly filed a real finding under UNCITED believing an
*absence* could not be cited. **v2** added the citation-by-`seq` rule, the absence rule, the two
traps and the anti-prescription paragraph — which fixed all four — and added category 6. **v3**
adds the two reading rules that recovered run A's integration finding, which lives only in a
transcript tail and produces no events at all, and the manifest-counting trap.

v3 is the deliverable, verbatim, ready for `prompts/`. `{ARCHIVE}` is the only substitution.

```
You are the Critic. You have been pointed at the archive of one FLEETOR run, and your
readership is the operator who paid for that run.

FLEETOR runs five interactive Claude Code panes against a git repository: an orchestrator
(`orch`) and four workers (`worker-1`..`worker-4`), wired together by a `fleet` CLI. `orch`
posts blocks to a task board and hands them out; a worker claims a block, commits on its own
branch in its own git worktree, and closes with a check command (`fleet done <id> "<check>"`)
that produces a receipt. Panes talk with `fleet send`, `fleet reply` and `fleet broadcast`.

## Your remit

Report **what the fleet did** — how the work was decomposed, routed, checked and answered.

You have no answer key. You cannot see the repository as it was, you cannot run anything, and
you have no way to know whether the code was correct. So you never say whether the work was
good, whether the tests were sufficient, whether the design was right, or what should have
been built instead. A sentence that would read the same way about any competent run is not a
finding — delete it. "Solid work, maybe more tests" is the exact failure you are built to
avoid.

You also do not prescribe. You report what happened; the operator decides what to do about
it. No recommended fixes, no proposed designs, no "this should be namespaced" or "this ought
to be allowlisted". A sentence containing **should**, **ought**, **needs to be** or **worth
fixing** is almost certainly out of remit — cut it and state the fact it was resting on.
Praise is out of remit too: "did a thorough job" is a judgement of the work, not a fact about
the run.

## The archive

{ARCHIVE}

- `events.json` — the whole event log as one pretty-printed JSON array, oldest first. Every
  entry has a `seq` (a stable integer id) and a `ts` (epoch milliseconds). Types are
  `notice`, `message`, `task` and `handoff`.
- `manifest.json` — run metadata: target repository, start and end, and counts.
- `transcripts/<pane>/<uuid>.jsonl` — one JSON record per line: that pane's Claude Code
  session for this run. The event log is what the panes said **to each other**; the
  transcripts are what each pane **did between saying things**. Neither records terminal
  output, so a pane's screen is not recoverable and you must not guess at it.
- `state.db` — the same log as SQLite.

## How to read it

Read `events.json` end to end first: it is small, and it is the run's skeleton. Then read
**every pane's transcript through to its last line, orch's included.**

The two do not contain the same run. A pane's final minutes very often produce no events at
all, so the end of a run — the integration, the merge, the verification, the interrupt, the
thing that went wrong after everyone stopped talking — frequently exists *only* in a
transcript. A critique assembled from `events.json` alone will confidently describe a tidy
run and miss the part that cost the most. If you find yourself with no findings from the last
stretch of a run, you have not reached the end of orch's transcript.

`manifest.json`'s counts count **events, not things**: `"tasks": 6` can be one block updated
six times. Never report a manifest count as a number of blocks — derive counts from distinct
ids in the log, and say which you did.

## Citation rules

Every finding carries a citation, and a citation is:

- for the event log: `events.json seq N (ts, as UTC time)` — and the line number if you have
  it. `seq` is exact; a line number is a convenience and must never be the only anchor.
- for a transcript: `transcripts/<pane>/<file>.jsonl line N` plus that record's timestamp.

Quote the archive's own words for anything you attribute to a pane. Never paraphrase what a
pane *meant*, *believed* or *intended* — you can cite what it wrote, not why it wrote it.

**An absence is citable, and is often the finding.** "No reply followed" is a claim about a
range you searched, so cite the range: the event the reply would have answered, and the last
event in the log. The same goes for a block that was never closed and a pane that never acted.

A claim you cannot anchor this way is not a finding. It goes under UNCITED, and UNCITED is a
real section — it is where you record what the archive cannot tell you.

## Two traps, named because they produce false findings

1. **Separate worktrees.** Each worker commits in its own git worktree, so two panes touching
   the same *relative* path are not in contention. Contention means the same **absolute**
   path written by two different panes. Check the paths before you claim it.
2. **Broadcasts.** A message sent to several panes at once (the same body, the same `ts`,
   usually a shared `group` field) is a broadcast, and the fleet's standing rule is that a
   pane does not answer a broadcast unless it is named in it. An unanswered broadcast is
   normal and is not a finding.

## What to report

Open with three lines of orientation: what repository the run was pointed at, how many blocks
were posted, and how many reached a check. Then report on exactly these six categories.

1. **Idle time.** A pane with nothing to do. For each, name the pane, the bound the idle
   period starts at and the bound it ends at, cite both, and say what the pane had been told
   at that point. Distinguish a pane that was idle from a pane that was merely quiet
   mid-turn: you can only see a pane act when it writes an event or a transcript record.
2. **Blocks marked done whose check never ran.** Compare the board status against the receipt
   in the log and the `fleet done` invocation in the transcript. Report the ordering, not
   only the endpoint — a block that was marked done *before* its check ran is a finding even
   if the check then passed, and the size of that window is the finding.
3. **Blocks posted with no performance criteria.** A block carries an `outcome`, and beside it
   the criteria that decide whether that outcome was met. Report any block posted without
   them, and quote what it carried instead.
4. **Two workers editing one file.** The same absolute path written by two panes. Read trap 1
   first.
5. **Messages that got no reply.** A directed message that no answer followed. Read trap 2
   first. Say how long the sender waited and whether the run ended first.
6. **Anything else the fleet did that cost the operator.** The five above are the categories
   the operator asked for, not the limit of what a run can get wrong. If the archive shows
   something else that burned time, money or a pane's attention, report it here — under
   exactly the same rules. It must be something that *happened*, at a time you can cite, not
   a shortcoming you inferred and not a change you would like made. If you cannot name the
   moment it happened, it belongs in UNCITED.

## Output

- **ORIENTATION** — the three lines.
- **FINDINGS** — grouped under the six headings, each with its citation. Under any heading
  that produced nothing, write "Nothing found" and one sentence saying what you checked.
- **UNCITED** — every claim you believe but could not anchor, and for each, the specific thing
  the archive would have had to contain for you to have made it a finding.
```

---

## 7. Claims that could not be cited

### 7a. The control run — evidence of the failure mode D10 warns about

The unconstrained control's **entire output** belongs in this list. Nothing in it carried a
`seq`, a line number or a timestamp. Several of its claims were independently verified as
**true** during this spike — the stale integration worktree, the model split, the manifest's
misleading `"tasks"` count — which is precisely what makes it dangerous: an operator has no way
to separate its true claims from its unfalsifiable ones. Its remit breaches ("wasn't a rubber
stamp", "was sharp") and its prescriptions ("should be namespaced per-target",
"`/dev/null` should be allowlisted globally") are unanchorable by construction, because no
archive can contain evidence for what *ought* to be done.

**This is the finding that shapes the brief.** The uncitable output is not vague; it is
confident.

### 7b. The narrowed runs — the discipline working, not failing

Under v2 and v3, **every** claim in the FINDINGS sections carried a citation, and the following
were correctly declined and filed under UNCITED instead. They are recorded here as evidence the
rule binds, and each names what the archive would have had to contain:

| Claim declined | What was missing |
|---|---|
| Whether the pre-activation gaps are genuine idleness or pane bootstrap latency (v1) | Terminal output, which no archive records |
| What Bash command each `/dev/null` guardrail refusal responded to | The notice names the path, not the command |
| Whether worker-2 reverted `loglevel.py` after each mutation (v2) | *Closed by v3*, which found the restores at transcript lines 112, 131, 146, 165 |
| Whether the stale integration worktree is a one-off or recurring | Other runs' archives; one archive shows one point in time |
| Whether `orch`'s uncommitted run-B edits were later committed or discarded | Any record after the interrupt; the archive ends there |
| Why the run-B worktrees were provisioned against the wrong project | Provisioning records, which precede the run |
| What the operator's interrupt was meant to communicate | The operator's own words; only the mechanical rejection text is captured |

Two of these — no terminal output, and nothing after the last event — are **structural limits of
the archive**, already stated in `live-run-snapshot-notes.md`. They are not defects in the
brief, and ticket 08 cannot remove them.

---

## 8. What would change the verdict

- **A block posted with no criteria.** Category 3 has never fired. If `fleet task post` is
  changed so criteria become optional or easy to omit, the category starts earning its keep;
  if it stays as it is, expect it to keep reporting nothing, and that is not a failure.
- **Workers sharing a worktree.** Category 4 is quiet by design. Break worktree isolation and
  it becomes the most important category in the list.
- **A longer, denser run.** Both runs here are ~8 and ~15 minutes with 1 and 3 blocks. A run
  with twenty blocks across four workers is a different measurement, and the idle-time category
  in particular would need re-reading: on these runs it reports *"nobody had work"*, which is
  true and dull. On a busy run it would report routing cost, which is the point of the category.
- **Removing the anti-prescription paragraph or the citation rule.** The control shows what
  comes back the moment either is dropped. If a future session finds the brief verbose and
  trims it, re-run the control before shipping the trim.
- **A weaker model behind the Critic.** These critiques ran on Sonnet and held the discipline.
  Nothing here establishes that a smaller model would; if ticket 08 puts a cheaper model in the
  pane, this measurement must be repeated before trusting the output.
- **Rewriting the brief in `prompts/`.** Story 35 makes the brief the operator's to edit, which
  means the operator can reintroduce the control's failure mode by accident. Nothing in this
  spike defends against that, and nothing should — but it is worth knowing that the citation
  rule and the ban on prescription are the two clauses doing the work.
