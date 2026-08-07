# WP-12 — The self-improving loop (the arc)

status: design size: XL — an arc, not a package
depends-on: 11 blocks: 13, 14, 15, 16, 17, 18, 19
brief-cost: 0 for this doc — the packages it files carry their own

**This is an arc doc, not an executable package.** Like `00-index.md` is the hub for the
Blackboard arc, this is the hub for the self-improvement arc: the design, the diagrams,
the invariant arguments already had, and the split into packages. The packages themselves
(WP-13..19) are written from `TEMPLATE.md` when each is picked up.

---

## Outcome

FLEETOR stops being a fleet that builds what it is told and becomes a fleet that gets
better at building. A fleet works a real mission against real code; an evaluator it
cannot see reads everything it did and walks the orchestrator back through its own
decisions; the proposals that survive that conversation become the platform's next
version. Over generations the fleet works out how to work — the decomposition, the
routing, the review calls, the prose in its own briefs — instead of being told.

The single property that makes this real rather than two Claude instances agreeing with
each other: **the evaluator holds an answer key the orchestrator has never seen.** The
mission is a feature a real project actually shipped, and upstream's own commit is what
the fleet's work is judged against. Without that asymmetry a retro converges on "solid
work, maybe more tests" and the cycle teaches nothing.

---

## Vocabulary

One name per concept, fixed here. A doc, prompt or commit that uses a different word for
one of these has the word wrong, not the concept.

| Name | What it is |
|---|---|
| **dev mode** | A user-triggered app mode, loud in the UI. Nothing about the evaluator exists outside it. |
| **the evaluator** | A real `claude` in its own window. Reads the run, coaches `orch`, never edits anything. |
| **build run** | The fleet doing the actual work against a target repo. The evidence-generating half. |
| **the retro** | The conversation after `orch` declares done: evaluator questions, orch answers, proposals come out. |
| **rewind mission** | A real repo pinned to an old tag, aimed at a feature upstream actually shipped next. |
| **the answer key** | Upstream's real commit for that feature. Not on disk until the retro starts. |
| **the proposal ledger** | Where proposals accumulate across cycles. Approve now, or defer until there is data. |
| **the write guardrail** | Built as WP-17 (D-065): a `PreToolUse` hook per pane refusing writes outside that pane's own roots. This doc called it *the fence* below; that name was already WP-08's private `HOME` (D-052), so the built thing took a name of its own. Writes only — reads stay open, deliberately. |
| **the veil** | The property that `orch` does not know the evaluator exists until the retro starts. |
| **the done verb** | Built as **`fleet handoff`** (WP-13, D-064) — `orch` declaring the goal met. Spelled `handoff` because `fleet done` was already the worker's block receipt. |
| **improve run** | A second fleet, pointed at FLEETOR's own source, implementing approved proposals. |
| **generation** | One promoted build of the platform. The boundary between generations is a relaunch. |

---

## The cycle

```
 ┌──────────────────────────────────────────────────────────┐
 │  REWIND HARNESS              scripts · outside the app   │
 │  ────────────────────────────────────────────────────    │
 │   • clone target repo, pinned to an OLD tag              │
 │   • derive the mission from what upstream shipped next   │
 │   • answer key  ░░░░░░░░░░  NOT ON DISK YET              │
 └────────────────────────┬─────────────────────────────────┘
                          │  mission
                          ▼
 ┌──────────────────────────────────────────────────────────┐
 │  THE FLEET                   FLEETOR as it works today   │
 │  ────────────────────────────────────────────────────    │
 │                                                          │
 │        orch ◄────────► worker-1  worker-2  worker-3  -4  │
 │         │               (own worktree, own branch each)  │
 │         └── task board · receipts · peer review          │
 └────────────────────────┬─────────────────────────────────┘
                          │  commits, receipts
                          ▼
               ┌─────────────────────────┐
               │      RUN ARCHIVE        │
               │   events.json           │
               │   transcripts/worker-N  │
               │   transcripts/orch  ◄── the gap WP-14 fixes
               └───────────┬─────────────┘
                           │
═══════════════════════════════════════════════════════════════
      orch calls the DONE VERB   ← the only trigger
      it thinks it is reporting to the operator
═══════════════════════════════════════════════════════════════
    everything below here was INVISIBLE to orch until now
                           │
            ┌──────────────┴──────────────┐
            │                             │
            ▼                             ▼
 ░░░░░░░░░░░░░░░░░              ┌──────────────────────┐
 ░ answer key    ░ ────────────►│   THE EVALUATOR      │
 ░ released now  ░              │   own window         │
 ░░░░░░░░░░░░░░░░░              │   brief baked into   │
                                │   the binary         │
                                └────┬────────────┬────┘
                                     │            │
                        the retro ◄──┘            │
                  ┌──────────────────┐            │
                  │  orch  ◄──────►  │            │
                  │  evaluator       │            │
                  │  (argues, both   │            │
                  │   can be wrong)  │            │
                  └──────────────────┘            │
                                                  ▼
                                      ┌───────────────────────┐
                                      │   PROPOSAL LEDGER     │
                                      │   surface + evidence  │
                                      │   + orch's position   │
                                      └───────────┬───────────┘
                                                  │
                                                  ▼
                                           ┌─────────────┐
                                           │  OPERATOR   │
                                           │  approve /  │
                                           │  defer /    │
                                           │  reject     │
                                           └──────┬──────┘
                                                  │ approved
                                                  ▼
                                      ┌───────────────────────┐
                                      │  IMPROVE RUN          │
                                      │  fleet points at      │
                                      │  FLEETOR's own code   │
                                      └───────────┬───────────┘
                                                  │
                                                  ▼
                                        next generation ──┐
                                                          │
 ┌────────────────────────────────────────────────────────┘
 └──► back to THE FLEET, running on the improved platform
```

**The double line is the design.** Two things cross it at the same instant and never
before: the evaluator becomes visible to `orch`, and the answer key lands on disk.

**The operator is the only writer.** The evaluator cannot edit. `orch` cannot self-modify.
Nothing reaches the platform without passing through the OPERATOR box — which is Tier 1.8
("shared knowledge merges only after review") applied to the platform itself.

### Who can see what

| | `orch` sees | the evaluator sees |
|---|---|---|
| Pinned repo | yes | yes |
| Answer key | **never** | after the done verb |
| Run archive | its own half | all of it, including `orch`'s transcript |
| Evaluator's brief | **never** — baked into the binary from a separate repo | it is its own brief |
| Proposal ledger | **never** | yes |

---

## The chicken-and-egg problem: a fleet that edits its own source

An improve run edits FLEETOR while FLEETOR is running. Five cases, four of which are
either free or already on the roadmap. **Case 2 is live today** — `npm run tauri dev`
runs Vite, so HMR will hot-swap the running UI under an improve run.

```
 ┌──────────────────────────────────────────────────────────────────┐
 │  IMPROVE RUN                                                     │
 │  2nd fleet, own worktree, pointed at FLEETOR's own source        │
 └────────────────────────────────┬─────────────────────────────────┘
                                  │
                  ┌───────────────┴───────────────┐
                  │  does this hit the LIVE app?  │
                  └───────────────┬───────────────┘
                                  │
     ┌───────────────┬────────────┼────────────┬──────────────────┐
     ▼               ▼            ▼            ▼                  ▼
┌─────────┐   ┌───────────┐  ┌──────────┐  ┌──────────┐   ┌────────────┐
│ CASE 1  │   │  CASE 2   │  │  CASE 3  │  │  CASE 4  │   │  CASE 5    │
│ Rust    │   │ UI source │  │ runtime  │  │ a broken │   │ fleet edits│
│ source  │   │ ui/src/** │  │ paths    │  │ change   │   │ EVALUATOR  │
│ crates/ │   │           │  │ .sock    │  │ lands    │   │ code       │
│src-tauri│   │           │  │ state.db │  │          │   │            │
└────┬────┘   └─────┬─────┘  └────┬─────┘  └────┬─────┘   └─────┬──────┘
     │              │             │             │               │
     ▼              ▼             ▼             ▼               ▼
   ═════          ═════         ═════         ═════           ═════
    NO             YES           YES          NOT NOW        SILENT
   ═════          ═════         ═════         ═════           ═════
     │              │             │             │               │
 compiled      tauri dev     both fleets    binary is       the GRADER
 binary is     HMR hot-      want the       fine — the      changes
 already       swaps the     SAME socket    NEXT launch     between
 loaded;       live UI       and the        is what         generations
 files on      under you     SAME db        breaks
 disk are
 inert
     │              │             │             │               │
     ▼              ▼             ▼             ▼               ▼
┌─────────┐  ┌────────────┐ ┌───────────┐ ┌────────────┐ ┌────────────┐
│  FIX    │  │    FIX     │ │    FIX    │ │    FIX     │ │    FIX     │
│ none    │  │ run a      │ │ WP-18     │ │ keep gen N │ │ evaluator  │
│ needed  │  │ BUILT      │ │ multi-    │ │ binary.    │ │ lives in   │
│         │  │ binary,    │ │ fleet is  │ │ rollback = │ │ separate   │
│         │  │ never      │ │ a hard    │ │ relaunch   │ │ repo, baked│
│         │  │ tauri dev  │ │ prereq    │ │ it         │ │ into binary│
└─────────┘  └────────────┘ └───────────┘ └────────────┘ └────────────┘


 ══════════════════════════════════════════════════════════════════
   THE RULE THAT DISSOLVES ALL OF IT
   the fleet is a CONTRIBUTOR, not a hot-patcher
 ══════════════════════════════════════════════════════════════════

        fleet edits            you rebuild          you relaunch
        in a worktree   ───►   + promote     ───►   ═════════════
        (own target/)          (the gate)           GENERATION
              │                     │                 BOUNDARY
              │                     │
              │              ┌──────┴──────────────────┐
              │              │  PROMOTION GATE         │
              │              │  ─────────────────────  │
              │              │  cargo test --workspace │
              │              │  tsc --noEmit           │
              │              │  vite build             │
              │              │  app reaches a prompt   │
              │              └──────┬──────────┬───────┘
              │                     │          │
              │                  PASS         FAIL
              │                     │          │
              │                     ▼          ▼
              │              ┌───────────┐  ┌──────────────┐
              └──── never ──►│ gen N+1   │  │ stay on      │
                    writes   │ goes live │  │ gen N binary │
                    into the └───────────┘  │ feed the     │
                    install                 │ failure to   │
                    location                │ the evaluator│
                                            └──────────────┘


 ┌──────────────────────────────────────────────────────────────────┐
 │  CASE 5 IN FULL — the confounder nobody would catch              │
 │  ──────────────────────────────────────────────────────────────  │
 │                                                                  │
 │   gen N   judged by  ──►  EVALUATOR v1  ──►  "6 findings"        │
 │   gen N+1 judged by  ──►  EVALUATOR v1  ──►  "2 findings"        │
 │                              ▲                                   │
 │                              └── SAME grader = real comparison    │
 │                                                                  │
 │   gen N   judged by  ──►  EVALUATOR v1  ──►  "6 findings"        │
 │   gen N+1 judged by  ──►  EVALUATOR v2  ──►  "2 findings"        │
 │                              ▲                                   │
 │                              └── did the fleet improve, or did    │
 │                                  the grader get soft? UNKNOWABLE  │
 │                                                                  │
 │   RULE: the grader is FROZEN across generations. Changing it      │
 │         deliberately resets the baseline — version-stamp it on    │
 │         every retro so you know which generations compare.        │
 └──────────────────────────────────────────────────────────────────┘
```

---

## Hiding: what must be absent, and what is merely fenced

**The honest limit, stated first.** A Claude Code process running as the operator's user
can read any file that user can read. `orch` has Bash. Deny rules and hooks raise the cost
of finding something; they do not make it unreachable — anything readable by
`python3 -c "print(open('...').read())"` is readable. **The only guardrail that enforces
is the file not being there.** Everything else is friction, and friction is genuinely
enough against the realistic threat, which is accidental discovery rather than an
adversary trying to cheat.

Three things contaminate if read. Everything else about this feature is harmless.

| Must be absent | Why | How |
|---|---|---|
| **The answer key** | `orch` reads it, `orch` implements it, the evaluation is void | Shallow clone pinned at the old tag — upstream's later commits are never downloaded. Fetched only after the done verb, into a directory no fleet is pointed at. **Free, and *not* airtight — see the correction below.** |
| **The evaluator's brief** | `orch` optimizes for the rubric instead of doing the work | Separate private repo, compiled in via `include_str!` behind a `devmode` cargo feature. This repo's source carries zero evaluator prose; no file on disk holds it in a dev build either. |
| **The ledger and past retros** | `orch` reading last cycle's critique pre-empts this cycle's finding | `~/.fleetor/dev/`, outside the repo, plus deny rules. Not airtight; the contamination cost is lower and post-hoc. |

Harmless if found, and therefore not hidden: dev-mode UI code, the second-window
plumbing, the done verb (`orch` is *taught* it), and the fact that an evaluator exists —
`orch` learns that at first contact anyway. **The feature is not hidden. The rubric, the
answer key and the history are.**

Compiling the brief in from a separate repo buys two properties at once: the fleet cannot
**find** it, and there is nothing on disk for the fleet to **edit**. That second property
is what freezes the grader across generations (Case 5 above).

### Correction (2026-08-07) — the answer key is absent, not unobtainable

This doc originally called the answer key "airtight and free." **It is free; it is not
airtight**, and the measurement that says so is `fleetor-eval/NOTES.md`. Two facts, both
measured rather than reasoned:

- The pinned tree **names its own upstream in six files and states its own version**, so
  `git clone <url> /tmp/x` retrieves everything the shallow clone withheld.
- GitHub serves arbitrary SHAs to `git fetch` (`allowAnySHA1InWant` is on), so knowing a
  SHA is sufficient — and the answer-key SHAs live in `missions/*.md`, readable by
  absolute path.

Scrubbing the upstream identity is not available as a fix: **the information that makes
the hole possible is the project itself.** A mission whose repo cannot be identified is
not a mission.

**What absence actually buys** is therefore the same thing it buys for the ledger, not the
stronger thing this doc claimed: it stops *accidental* discovery, and it makes deliberate
contamination require an act that leaves a trace in the archived transcript. **The
mitigation is a post-hoc audit, not network egress control** — a clone of upstream, or a
fetch by SHA, appearing in the run's transcripts voids that run. That check is cheap, it
is the evaluator's job, and it is honest about what it is. `verify-absence.sh` prints this
limit as its own section on every run, because 31 green PASSes otherwise read as a
guarantee the mechanism cannot give.

### Correction (2026-08-07) — freezing the grader is not sufficient; the mission must be fixed too

This doc says the grader must be frozen across generations (Case 5). That is necessary and
**not sufficient.** Judging weights are per-mission, so a generation comparison holds only
if the mission is fixed as well — and **the same mission run twice is contaminated**,
because generation N+1's fleet can inherit context, branches and archived reasoning from
generation N's attempt at it.

So a real generation comparison needs **pairs of missions of matched difficulty**: N runs
mission A, N+1 runs mission A′. Building a matched pair is materially harder than building
two good missions, and nothing in the arc currently does it. **This is an open problem, not
a solved one** — the honest fallback until it is solved is that generation-over-generation
claims are qualitative, and only within-generation findings rest on measurement.

### The deny layers, ranked by whether they enforce

1. **Absence** — the three rows above. Real.
2. **Managed settings** — a settings layer a pane cannot override by editing its own
   config. *Unverified: the exact path and precedence for the installed CC version has
   not been checked. Verify before relying on it.*
3. **A `PreToolUse` hook** — inspects the actual Bash command string rather than a glob,
   so it catches `cat`/`python`/`find` variants pattern rules miss. **Verified at CC 2.1.224
   and built as WP-17** (D-065): a deny really does stop the tool call, it beats an explicit
   `--allowedTools` allow, and the reason reaches the model as words it acts on
   (`docs/notes/write-guardrail-notes.md` §1). What it enforces is *writes*; and for `Bash`
   only what a command actually **names**, which is why `cargo build` and `git commit` still
   work — measured, §3 of the same file.
4. **The Fence** (D-052, built) — workers get a private `HOME`, so `~/.fleetor` does not
   resolve by name. Absolute paths still do; `docs/notes/fence-notes.md` says so outright.

**Where `orch`'s deny rules live is the reason WP-14 moved earlier.** `orch` uses the
operator's own config dir today, which is not somewhere this app may write policy. A
per-run `CLAUDE_CONFIG_DIR` for `orch` gives the deny rules and the hook a home — one
change, two jobs.

---

## The streams

Four streams. **Stream 3 touches no product code and can start immediately**, in parallel
with anything. Stream 4 is a large independent feature the loop needs only at its last
step.

| WP | Stream | Name | Depends on | Core? |
|----|--------|------|-----------|-------|
| 13 | 1 — the retro loop | The done verb | — | yes — verb + both briefs, one commit |
| 14 | 1 | `orch`'s own transcript + config dir | — | yes — `spawn.rs`, `runs.rs` |
| 15 | 1 | The evaluator window | 13, 16 · soft 14 | yes — 2nd window, `PaneId` |
| 16 | 2 — dev mode & the fence | Dev mode | — | no |
| 17 | 2 | The write guardrail (`17-write-guardrail.md`, landed) | 14, 16 · **blocks any improve run** | yes |
| 18 | 4 — multi-fleet | More than one fleet at once | — | yes |
| 19 | 4 | Orch-to-orch, and its cutoff | 18 | yes |
| — | 3 — rewind evidence | The rewind harness + mission bank | — | **no — imports nothing from the app** |

Stream 3 has no WP number on purpose: it is not a package against this codebase. It is a
separate repo, and it is the largest chunk of the work that cannot cause core churn at
all. That property is deliberate — see "the discipline" below.

### The proposal ledger

Filed under WP-15's stream but built after it, since it only has content once retros
produce proposals. Each proposal carries four fields:

| Field | Example |
|---|---|
| **Surface** | orch's prompt / a worker's prompt / a convention / a platform change |
| **Evidence** | "9-min block at 14:42, and the same pattern in runs 3 and 5" |
| **What it would change** | "brief clause: hand a worker the interfaces it will need, not just its job" |
| **What would falsify it** | "next three runs still show cross-worker blocking on details orch held" |

Three operator moves: **approve** (goes to the next improve run), **defer** (accumulates —
three cycles independently proposing the same thing is data, and is how "build A and B,
not D and E" gets decided), **reject** (recorded with a reason so it stops being
re-proposed).

---

## Invariant guardrails

Four places this arc meets something `building.md` §1 already decided.

### Tier 1.4 — nothing in the message path. The orch-to-orch cutoff cannot be a mute.

Tier 1.4 forbids anything between `fleet send` and a pty that can delay, refuse, reorder,
drop or silently alter a message; §9.3 records that it has been argued and lost twice. A
cross-fleet kill switch built as "accept the send, then drop it", or as a mute flag
checked before writing to the pty, is exactly the banned thing — and it would make the log
lie, which L3 calls the worst failure mode this product has.

**The allowed shape: make it addressing, not delivery.** When the link is off, the other
fleet's panes are *not addresses*. `fleet send other:orch` fails at **resolution** with
"no such pane", the same way `fleet send worker-9` fails today. The message never enters
the delivery path, so nothing in the path refuses it, and the CLI honours the switch for
free because it is asking the hub to resolve a name that does not exist. Turning the link
on adds addresses; turning it off removes them. **WP-19 must be built this way or filed as
a Tier-1 question.**

### Tier 1.3 — every pane is a real interactive TUI. Satisfied, with one real cost.

The evaluator is a real `claude` on a real pty in its own window — typeable, plan mode
works, skills work. What is new is that §7 rule 5 ("every terminal stays mounted; unmount
destroys the buffer") now needs a second terminal host outside the grid the dashboard band
assumes. That is the price of the separation, and it is the honest one: an evaluator
inside the grid would read as a fifth worker.

### Tier 1.7 — auto-approve never exceeds the worker's worktree. The fence is not optional.

Workers spawn `--permission-mode auto`, scoped to their own worktree. During an improve run
that worktree **is** a checkout of this repo — so auto-approve legitimately covers the
dev-mode code and the evaluator's source. Tier 1.7 is not violated; it stops being a
meaningful limit for this one case. WP-17 is the only thing standing between an
auto-approving worker and the evaluator's own code, and it lands **before** the first
improve run, not after the first accident.

### Tier 1.8 + D-030's regrowth warning — the coach that was rejected once already.

Tier 1.8 ("shared knowledge merges only after review") is honoured by the operator being
the only writer in the loop. Separately: a coach pane was tried and abandoned on
2026-08-06 — not because the idea was wrong, but because a test rig grew into ~10 files of
core churn while presenting itself as a test rig. The operator's verdict was "too polluted."

**The discipline that stops the repeat:** this is product from the first line, gated behind
a mode the operator can see; every stream ships as its own package with its own exit
criteria; and Stream 3 — the largest chunk of the work — imports nothing from the app, so
it cannot cause core churn at all.

---

## Sequence

1. **Stream 3 — the rewind harness** *(parallel, start now)*. Zero product code, so it
   breaks nothing and needs no decision from the streams above. It answers the question
   everything else rests on: does pinning a real repo to an old tag actually produce a
   mission worth running?
2. **WP-16 — dev mode** *(parallel)*. Small, self-contained, and the container every later
   piece lives inside.
3. **WP-13 — the done verb.** The wake signal. Verb and both briefs in one commit per the
   contention rule in `00-index.md`. Useful alone before anything listens for it.
4. **WP-14 — `orch`'s transcript and config dir.** Before the evaluator, not after. An
   evaluator that cannot read `orch`'s reasoning critiques the fleet's messages and misses
   the decisions — and nobody would know that was why.
5. **WP-15 — the evaluator window.** The first thing to react to, and where it becomes
   clear whether a retro is genuinely useful or merely plausible-sounding.
6. **WP-17 — the fence, then the proposal ledger.** The fence before any fleet is pointed
   at this repo.
7. **WP-18 → WP-19 — multi-fleet.** The largest arc and the last the loop needs. Until it
   lands, an improve run is a manual re-point and a restart — enough to close the loop by
   hand and learn whether automating it is worth the work.

---

## Open questions

1. **The evaluator's brief prose.** Deliberately unwritten. It is the highest-leverage
   text in this design — it decides whether a retro produces "good job, consider more
   tests" or a critique `orch` has to argue with — and it should be written **against a
   real archived run**, not in advance. Two constraints already settled: it requires a
   citation (timestamp, file, line) for every claim, so "the decomposition felt rushed"
   cannot be written down; and it is told to find the strongest case *against* the run,
   because a neutral evaluator reading successful-looking output writes a congratulation.
2. **CC mechanism verification.** Managed-settings precedence and `PreToolUse` interception
   of `Bash` (deny layers 2 and 3 above). Both unverified; both change how much that layer
   is worth building.
3. **Which real repos become rewind missions.** Needs licence-compatible, well-tested
   projects with clean feature commits. Stream 3's first job.
4. **Does `orch` know it is in dev mode at all?** The veil covers the evaluator, not the
   mode. An `orch` that can see a dev-mode banner may infer the rest. Currently unresolved;
   the cheap answer is that dev mode is visible to the *operator's* UI and absent from
   `orch`'s brief and roster.
