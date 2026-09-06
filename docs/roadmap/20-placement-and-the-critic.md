# WP-20 — Placement, and the Critic (the arc)

status: design size: L — an arc, not a package
depends-on: — blocks: 21, 22, 23
brief-cost: 0 for this doc · 0 for WP-21 and WP-22 · WP-23 adds a **new** brief file and
touches neither `orch.md` nor `worker.md`, so the shared budget (D-053, orch 3,668 / cap
3,800) is untouched by the whole arc.

**This is an arc doc, not an executable package**, in the mould of
[`12-self-improving-loop.md`](./12-self-improving-loop.md). It carries the problem, the
decisions and the seam; the three packages it files — WP-21, WP-22, WP-23 — are written
from [`TEMPLATE.md`](./TEMPLATE.md) when each is picked up.

Origin: an architecture review on 2026-09-01 and the design session that followed it.

---

## Problem Statement

**For the operator.** When a pane comes up wrong, nothing tells you. A pane whose
configuration was seeded for the wrong working directory still opens, still shows a
terminal, and still accepts every message — into a trust dialog it will never leave.
`fleet send` answers `accepted`, honestly, because the bytes did reach a live pty. So
`orch` — running the operator's own Opus, on the operator's own money — believes the
worker has its task, waits, follows up, re-sends, and reasons about why that worker is
quiet. The failure is invisible from inside the fleet by construction, because Tier 1.5
forbids the system from claiming more than `accepted` ever means.

**For the maintainer.** Bringing one pane up is a ten-step sequence with a strict order:
resolve the working directory, seed the pane's configuration for that exact directory,
seed a worker's private `HOME`, mirror the Rust toolchain, install the write guardrail,
lay out the run for a reader, render the brief, record the gauge source, build the
command. Three of those modules — the one that shapes a process, the one that installs
the guardrail, the one that knows about the evaluator — are individually sound. **No
module owns the order they go in.** So the order is improvised at every call site: once
in the orchestrating module as a 135-line match with three shapes and no tests, and once
again in the write-guardrail test, which needed the sequence, could not call it, retyped
it by hand — and left a step out. The step it left out is the evaluator's narrower
guardrail roots, which is the veil. Nothing tests the veil.

It cannot be tested, either. Every path bottoms out in a function that reads the
operator's real home directory at call time and takes no argument, so the sequence can
only ever be run against the operator's own installation. Half this backend already
solved that for itself — the run archive, dev mode and the orphan sweep all take their
root as a parameter and are tested against scratch directories. The spawn path is the one
part that never got the same treatment, and it is the part where a mistake costs the
most: it is the most-changed source file in the repository.

**For the operator again, on the evaluator.** The evaluator wakes only when `orch` calls
`fleet handoff`, and it wakes in a second operating-system window. That means there is no
way to ask for a critique of a run that has already finished, no way to ask for one during
ordinary work on a real repository, and no way to iterate on the evaluator's own brief
without completing a whole mission first. The second window also carries a class of hazard
the rest of the application does not have: its close is intercepted and turned into a hide
so its terminal buffer survives, its capabilities are scoped by window label, and the
window-event handler once tore down all six ptys because it could not tell one window from
another.

**And a gap the self-improvement arc never named.** The evaluator is an instrument in an
experiment: it holds an answer key — what upstream actually shipped — that `orch` has
never seen, and that asymmetry is the only thing separating a real assessment from two
Claude instances agreeing with each other. But an operator doing ordinary work on their own
repository has no answer key and never will. There is no upstream commit to be right.
They still want to know how their fleet actually worked. Today there is nothing for them.

---

## Solution

Three changes, shipped in order, each useful on its own.

**One module owns bringing a pane up.** A placement module takes a description of the pane
to place, a layout value saying where this fleet lives on disk, a host value saying what
this machine has, the target, and the resolved briefs — and returns the command to run,
the notices the operator should see, and the gauge source to record. The order lives
inside it. Its filesystem effects are confined to the layout it was handed, so the same
function that runs in production runs in a test against a scratch directory. The
write-guardrail test deletes its hand-copied ritual and drives the real one, so the copy
cannot drift again. Nothing about the operator's experience changes.

**The evaluator becomes a view, not a window.** It moves into the main window's rail
alongside Terminals, Messages, Tasks, Activity and History. The rule that a terminal is
never unmounted is honoured by the same mechanism every other view already uses. The
second window, its label branch, its close interception, its per-label capability scoping
and its teardown hazard all go away.

**A second run-reading pane, for ordinary work.** The Critic reads a run — the live one, or
any archived one picked from History — and reports what the fleet actually *did*. It is
triggered by the operator, not by a handoff. It exists whether or not dev mode is on. Its
brief lives in `prompts/` where the operator can rewrite it, unlike the evaluator's, which
is compiled in from a separate repository precisely so the fleet can neither find nor edit
it. And it never messages the fleet: its findings go to the operator, who forwards
anything worth acting on through the composer they already have.

The two panes answer different questions, and that is why they are two panes and not one
pane with a flag:

```
EVALUATOR   has the ANSWER KEY   →  did the fleet build the right thing?
            an instrument: frozen, hidden, dev mode only, woken by the handoff,
            argues with orch.  Its output is a MEASUREMENT, and a measuring
            device you can adjust measures nothing.

CRITIC      has the LOG          →  how did the fleet actually work?
            a tool: tunable, always available, triggered by the operator,
            talks only to the operator.  Its output is an OPINION — but every
            claim must cite the archive, so it cannot be a vague one.
```

---

## User Stories

### The operator, on panes that come up wrong

1. As an operator, I want a pane that failed to be seeded correctly to be impossible rather than silent, so that I do not pay `orch` to coordinate with a terminal that is not listening.
2. As an operator, I want to switch the target repository and relaunch without any pane landing on a trust dialog, so that a target change is an ordinary act rather than a hazard.
3. As an operator, I want to be told once, clearly, when the `fleet` binary is not on the path, so that I learn it before a pane discovers it mid-turn as a command-not-found.
4. As an operator, I want to be told once, clearly, when no Rust toolchain is reachable, so that I am not debugging a worker's PATH when the real answer is that this machine has no rustup.
5. As an operator, I want a worker that cannot get its own worktree to say so and name what is degraded, so that I do not treat a reviewed `done` as reviewed when peer review is not actually possible.
6. As an operator, I want the loudness of a warning to match its cost, so that the notices I read are the ones that change what I do next.

### The maintainer, on the sequence

7. As a maintainer, I want one module to own the order a pane is brought up in, so that adding a step means editing one place rather than every caller that improvised the sequence.
8. As a maintainer, I want the preconditions between steps to be enforced by the shape of the code rather than by doc comments, so that a future session cannot get the order wrong quietly.
9. As a maintainer, I want to run the whole bring-up sequence against a scratch directory, so that I can test it without touching my own installation.
10. As a maintainer, I want a test that proves configuration is re-seeded for a new working directory after a target switch, so that the invariant the code calls structural actually has a test behind it.
11. As a maintainer, I want a test that proves a worker's `HOME` is the fleet's private one and that no inherited API key survives into its environment, so that the Fence is verified at the command rather than only at the seeding.
12. As a maintainer, I want a test that proves a machine with no rustup produces a command with those settings *absent* rather than pointing at nothing, so that the newest and least-exercised branch has coverage.
13. As a maintainer, I want a test that proves the evaluator's guardrail roots are narrower than any other pane's, so that the veil is a checked property rather than a comment.
14. As a maintainer, I want the write-guardrail test to drive the real bring-up sequence instead of its own copy, so that the copy cannot silently diverge from what production does.
15. As a maintainer, I want adding a new kind of pane to cost one variant rather than a new arm in a long untested match, so that the file that has changed most often stops growing.
16. As a maintainer, I want the module to have no process-global reads at all, so that tests do not have to mutate environment variables that other tests in the same binary are reading.
17. As a maintainer, I want a value that names where this fleet lives on disk, so that the four places that independently re-derive the same directory layout can converge on one.
18. As a maintainer, I want a value that names what this machine has, so that a machine without rustup, without a built `fleet` binary or without an API key is a case a test can express.
19. As a maintainer, I want this refactor to change no behaviour at all, so that I can review it by reading the diff and running the suite.
20. As a maintainer, I want the comment claiming the evaluator wake and the spawn path share one target to become true, so that a doc comment stops asserting something the code does not do.

### The operator, on where the evaluator lives

21. As an operator, I want the evaluator in a tab rather than a second window, so that it lives where every other part of the application lives.
22. As an operator, I want its terminal and scrollback to survive switching away and back, so that I can read the rest of the application without losing a retro in progress.
23. As an operator, I want closing the application to be unambiguous, so that no window's close can tear down more of the fleet than I meant.
24. As an operator, I want the evaluator tab to be absent outside dev mode, so that the application does not advertise machinery that has no meaning for the work I am doing.
25. As an operator, I want the evaluator tab to tell me it wakes on a handoff and offer no button, so that I can see the sequencing is deliberate rather than missing.

### The operator, on the Critic

26. As an operator, I want to ask for a critique of the run that is happening now, so that I can find out how the fleet is working without waiting for it to finish.
27. As an operator, I want to ask for a critique of any past run from History, so that I can learn from a run I have already closed.
28. As an operator, I want the Critic available in ordinary work with no dev mode and no rewind mission, so that it is a feature of the product rather than an instrument of an experiment.
29. As an operator, I want every finding to cite a timestamp, a file and a line in the archive, so that I can check it instead of taking it on faith.
30. As an operator, I want the Critic to report what the fleet *did* and not opine on whether the code is good, so that it does not waste my attention on judgements it has no grounds for.
31. As an operator, I want to see idle time, blocks marked done whose check never ran, blocks posted with no performance criteria, and two workers editing the same file, so that I learn where the decomposition and the routing actually cost me.
32. As an operator, I want the Critic to be a real, typeable terminal, so that I can argue with a finding or ask it to look again.
33. As an operator, I want the Critic to be unable to message the fleet, so that nothing it thinks can reach a working pane without passing through me.
34. As an operator, I want to forward a finding to `orch` myself using the composer I already have, so that acting on a critique needs no new mechanism and no new verb.
35. As an operator, I want to rewrite the Critic's brief, so that I can tune what it looks for as I learn what is useful.
36. As an operator, I want the Critic to be able to write only inside its own directory, so that a pane reading my run cannot change it.
37. As an operator, I want the Critic and the evaluator both available in dev mode, so that I can see how the fleet worked and whether it built the right thing as the two separate questions they are.
38. As an operator, I want the Critic never to appear in `fleet roster`, in a broadcast, or in any brief's peer list, so that it is a tool I hold rather than a participant the fleet has to account for.

### The fleet, and the veil

39. As `orch`, I want my brief unchanged by all of this, so that no tokens are spent teaching me about machinery I do not use.
40. As `orch`, I want the evaluator to remain absent from every enumeration I can read, so that the veil holds exactly as it did before.
41. As a worker, I want the word "reviewer" to keep meaning the peer who reads my commit, so that a new pane does not collide with the one convention my brief teaches me about review.
42. As the evaluator, I want my brief to stay compiled in from a separate repository with no override path, so that I stay frozen across generations and the fleet cannot optimise for my rubric.
43. As the evaluator, I want to remain the only pane with an answer key, so that a generation comparison keeps meaning something.

### The builder of the Critic's brief

44. As a maintainer, I want to hand-run a critique against a real archived run before writing any code, so that I learn whether a judge with no answer key finds anything worth reading.
45. As a maintainer, I want the prompt that spike converges on to become the shipped brief, so that the highest-leverage prose in the feature is written against evidence rather than in advance.
46. As a maintainer, I want the spike's findings recorded as a measurement note, so that a later session can see what was actually observed rather than what was hoped.

---

## Implementation Decisions

Fifteen decisions, taken in a design session on 2026-09-01. Numbered as they were taken.

### Scope and shape

**D1 — The missing thing is a session value, not a phase machine.** A layout value naming
where this fleet lives on disk, plus a placement module owning the per-pane sequence. The
fleet's live state already holds the target, the resolved briefs and the gauge sources; it
gains the layout.

*A lifecycle state machine was considered and ruled out by the code.* Pane identity already
derives every asymmetry from one fact per name rather than from a mode — the operator is
the one name with no terminal, the evaluator is the one name with a terminal that is not a
member of the fleet — and the module that owns that says outright it is "a property of the
name, not of how the app was started or of what happens to be spawned." The veil is not
"`orch` cannot see the evaluator *yet*"; it is "the evaluator is in no enumeration, ever."
That is stronger than a phase flag, because a mode can be wrong and a property of a name
cannot.

**D2 — Placement owns laying out the run for a reader.** The evaluator's working directory
*is* a snapshot of a run, so it cannot be placed without the archive step. One rule with no
exceptions: everything a pane needs before it exists is inside placement. The alternative —
the caller lays the directory out and passes a rendered brief in — reintroduces exactly what
this removes, a piece of the sequence living in the caller plus a parameter two of the
three arms discard.

**D3 — The module is named for placement.** "Launch" was rejected: a launch configuration
type and a launch settings file already exist for the flags a pane starts with, and two
things named alike in two crates is the ambiguity a prior decision spent an entry
untangling for the Fence and the write guardrail. Growing the process-shaping module was
also rejected: it would pull the run archive and the guardrail into a module that is
currently only filesystem seeding and command building.

**D8 — A placement description carries per-kind inputs.** The pane identity stays what it
is — a frozen wire type serialising as a bare string, shared by the CLI argument, the
database payload, the event field and the TypeScript mirror. The placement input is a
separate type, internal to the module, where each kind carries exactly what placing it
needs. This came out of the prototype and encodes the decision more precisely than prose:

```
enum PaneSpec {
    Orch,
    Worker(slot),
    Evaluator,                 // the live run, at handoff
    Critic { run: RunSource }, // live, or a named archived run
}

enum RunSource { Live, Archived(id) }

place(spec, &layout, &host, target, &context) -> Result<Placed>

struct Placed {
    command,   // what the registry will spawn
    notices,   // (level, text) pairs for the Activity feed
    gauge,     // the transcript source to record, for workers
}
```

No arm discards a parameter, and a future pane kind with its own input costs one variant
rather than another ignored argument.

### Removing the process-global reads

The module is only testable if nothing inside it reads the process. Four groups were found.

**D11 — Dev mode is read through the layout.** The evaluator arm must re-check the mode
itself rather than trust its caller; the existing code argues this explicitly, that
"unreachable outside dev mode has to be a property of the spawn site and not of its one
caller." Reading the stored flag through the layout keeps all three properties at once: the
spawn site genuinely re-checks and a caller cannot lie to it, the documented "read fresh,
every time" behaviour survives, and a test can set the flag in its own scratch
configuration. Passing the mode in as a boolean was rejected for losing the first of those.

**D12 — Discovery gets a value of its own, beside the layout.** The layout answers *where
this fleet writes*; a host value answers *what this machine has* — the operator's Rust
toolchain, the built `fleet` binary, the worker API key, the mission harness roots, and the
fake-pane override. Two concepts, two names. Folding them together was rejected: a layout
that also holds a secret is no longer a layout, and a path and a credential have different
lifetimes.

**D13 — The host is discovered per spawn, not once at bootstrap.** This is what today's
code does — the toolchain, the binary and the key are all resolved at each spawn — and
preserving it is what keeps WP-21 a pure refactor. It is also the better semantics:
installing a toolchain or adding an environment file mid-session takes effect on the next
pane restart, as it does now, rather than being invisible until relaunch. The cost is a
handful of filesystem checks, six times a session.

Each value has two real adapters, so neither seam is hypothetical: the layout is built from
the operator's home in production and over a scratch directory in a test; the host is
discovered from the machine in production and constructed bare in a test.

### Correctness carried alongside

**D14 — The wake and the spawn path read one target.** A doc comment asserts that the
evaluator wake uses the target snapshotted at bootstrap, "the same one the spawn path
renders the brief against." The reasoning is right and the code does not do it: the wake
carries its own copy while the spawn path reads the live field, which the target-setting
command mutates. WP-21 makes both read one source, which changes no behaviour and makes the
comment true. Separately, and as its own entry because it *is* a behaviour change: the
target-setting command should refuse once a pane exists, since the rule is currently
enforced only by the interface hiding an input.

### The evaluator, and the Critic

**D4 — The evaluator becomes a view.** The stated reason for the second window is that
mounting the main application root twice would give the app two webviews racing for one
fleet — an argument for the second window having its own root, not for the second window
existing. A view inside the single root has no such race. The rule that terminals are never
unmounted is already honoured for every existing view by hiding rather than unmounting, so
a view holding the evaluator's terminal satisfies it identically. This supersedes the part
of D-066 that chose a window, and removes the label branch, the close interception, the
per-label capability scoping and the window-event teardown hazard. The remaining objection
on record — that an evaluator inside the terminal grid would read as a fifth worker — is
answered by it being its own view rather than a tile in the grid.

**D5 — Two pane identities, not one with a mode flag.** The grader's brief must be frozen
across generations and unreachable by the fleet, which is why it is compiled in from a
separate repository. A critic for ordinary use must be editable by the operator, which
means its brief is a file in the prompts directory. Those requirements are opposites, so
they cannot be one identity switched by a flag — and a veil test that read "the fleet must
never name the evaluator, unless it is the other kind of evaluator" is precisely the
ambiguity the one-name-per-concept rule exists to prevent.

**D6 — The Critic reports to the operator and never messages the fleet.** Findings reach
the operator through its own view and the Activity feed. Anything worth acting on the
operator forwards as an ordinary send, using the operator-as-participant path that already
exists. This costs no brief text — which matters, since the orchestrator brief sits at
3,668 tokens against a 3,800 cap and every verb-adding change has landed between 3.8× and
4.6× over its estimate — adds no verb, and keeps the human as the only writer, which is the
same principle the self-improvement arc applies to the platform.

**D7 — The pane is called the Critic.** "Reviewer" is taken, and taken in prose the models
read: the orchestrator brief instructs `orch` to name a reviewer when handing out a block,
and the worker brief tells a worker to ask their reviewer to look. A pane named for review
sitting beside that convention is a collision inside the briefs themselves. "Observer" is
taken twice — by a browser API in the interface code and by a concept already rejected on
the roadmap. "Critic" is unused anywhere in the repository and matches the job as the arc
already states it: find the strongest case against the run. The evaluator's terminal is
currently labelled "review" in the interface; that label is retired as part of the same
change.

**D10 — The Critic judges the run, not the work.** It has no ground truth about whether the
code is right, and the arc warns that a judge without an answer key converges on "solid
work, maybe more tests." But the archive is full of hard facts about how the fleet worked:
idle time, blocks marked done whose check never ran, blocks posted without performance
criteria, two workers editing one file, messages that got no reply. None of those is an
opinion. So the brief is narrow on purpose — critique the run, cite the archive for every
claim, never grade the code — which borrows the citation discipline already settled for the
evaluator's brief, where a claim that cannot be pointed at cannot be written down.

*This decision closed an open question.* Whether the Critic should be hidden in dev mode
was a worry that two tabs both "critiquing a run" would confuse which one held the answer
key. With the remits separated they answer different questions, so both are visible in dev
mode and having both is clarifying.

### Shipping

**D9 — Three packages, in order.** Each has its own exit criteria and can be abandoned
without stranding the others. The repository has a recorded failure of exactly the shape
this arc could take: a coach pane abandoned because "a test rig grew into ~10 files of core
churn while presenting itself as a test rig," and the discipline named as the fix is that
every stream ships as its own package.

- **WP-21 — placement.** The layout and host values, the placement module, the sequence
  moved inside it. A pure refactor: no behaviour change, and the write-guardrail test drops
  its hand-copied ritual.
- **WP-22 — the evaluator becomes a view.** Small, mostly deletion, supersedes part of
  D-066. Also fixes the view-persistence list, which is already wrong today and gets wronger
  with a new view.
- **WP-23 — the Critic.** By this point it is one placement variant, one brief, one view and
  one action on the History rows.

**D15 — The Critic's brief is spiked before it is built.** `building.md` requires that
anything touching an unknown gets a throwaway first and that the measurement goes in the
docs, and the unknown here is whether a judge with no answer key produces anything worth
reading. The spike costs no code at all: point an ordinary session at an archived run
directory — which is already written to be read by an agent, transcripts included — and ask
for a critique under D10's rules. The findings become a measurement note; the prompt that
converges becomes the shipped brief; and if it produces mush, that is learned for the cost
of one session rather than a package.

---

## Testing Decisions

### What makes a good test here

A good test drives the interface a caller uses and asserts what a caller can observe — the
command that will be run, the files that were written, the notices that came back — and
never reaches past the interface to check how the module arranged its own work. This is the
distinction the current suite fails on: the seeding functions have unit tests, and the
sequence that calls them has none, so what is verified is each ingredient and never the
recipe. Prose invariants inside a long function are promises with no enforcement; this arc
is largely about converting three of them into assertions.

### One new seam

**The placement function is the seam, and it is the only new one.** It takes values and
returns a value, and every filesystem effect is confined to the layout it was handed, so
the identical code path that runs in production runs in a test against a scratch directory.
Every claim across all three packages is assertable there:

| Claim | Today | Through the placement seam |
|---|---|---|
| Configuration is re-seeded after a target switch | untested | place for one target, then another; both project keys present |
| A worker's `HOME` is the fleet's private one, and no inherited API key survives | seeding tested, command not | assert the environment on the returned command |
| No toolchain means those settings are absent, not empty | untested | a bare host value; assert the settings are missing |
| The evaluator's guardrail roots are narrower than any pane's | untested | read the policy file that was written |
| Dev mode off means no evaluator | only at the readiness check | placing an evaluator errs |
| A target that is not a repository degrades to a shared checkout, loudly | wording pinned, trigger untested | assert the returned notices |
| The Critic gets the same narrow roots and its own brief | n/a | the same assertions as the evaluator |

Two adapter pairs make that possible, and each has a genuine second adapter rather than a
hypothetical one: the layout is built from the operator's home or over a scratch directory;
the host is discovered from the machine or constructed bare.

### Existing seams kept, and what changes about them

**Prior art is the point** — three of the four are patterns already in the suite.

1. **Executing the installed guardrail hook.** The write-guardrail test runs the real hook
   through a real shell against real tool-call payloads, with no model and no tokens. That
   *decision* seam is kept entirely. Only its **setup** changes: it stops retyping the
   bring-up sequence and drives the placement function instead. This is the single most
   valuable change in WP-21, because the hand-copied setup is what dropped the evaluator's
   narrow roots and is why the veil has no test.
2. **Real ptys with a stand-in pane.** The pane test spawns five real ptys against a
   five-line script and proves channel isolation, ordering and reaping. Untouched —
   placement stops short of the registry, and the registry keeps taking a prepared command
   and knowing nothing about how a pane is shaped.
3. **Counting application commands.** The handoff test counts the commands a handoff causes
   and expects zero, which is how Tier 1.4 is held behaviourally rather than by inspection.
   Untouched, and the model for anything new that must prove an absence.
4. **Source-reading tripwires.** The dev-mode and evaluator tests pin the absence of
   coupling by reading source. They are kept and **extended with care**: the veil tripwires
   must keep matching the evaluator only, and must not begin matching the Critic, which is
   openly named in the interface and in its own brief. A tripwire that quietly started
   covering the Critic would report the veil as broken when it is not.

### Modules tested

The placement module through its seam; the guardrail through the hook it installs, set up
by placement; the layout and host values through their scratch-directory constructors. The
existing seeding, command-building, readiness and archive modules keep their own unit
tests, now reached as internals rather than as public entry points.

The worker arm's successful worktree path shells out to git, so a test of it needs a real
repository in a scratch directory — an initialised repo with one commit. The fallback path,
where the target is not a repository at all, needs nothing. Running real subprocesses is in
keeping: the suite already spawns five real ptys and executes a real hook through a real
shell.

### No new seam in the interface layer

The frontend has no test infrastructure — no runner, no test script, and four thousand
lines covered only by the type checker. This arc deliberately does not change that. WP-22
and WP-23's interface changes are verified by hand and by the placement seam underneath
them. Introducing a frontend runner is worthwhile on its own merits and belongs to its own
piece of work; doing it as a side effect of a package about moving a terminal into a tab is
how a small package becomes the polluted one.

One cheap exception is worth considering in WP-22: a source-reading tripwire asserting that
the list of views the interface declares and the list it will restore on relaunch name the
same things. That list is already wrong today — one existing view is missing from it, so
selecting it and relaunching silently lands elsewhere — and this arc adds two more views to
get wrong.

---

## Out of Scope

- **A relocatable state root.** Making the fleet's directory configurable is an
  operator-facing feature nobody has asked for, and it would make the boundary test —
  removing one directory undoes everything — false or in need of rewording. What this arc
  needs is the root arriving as an argument with one production call site that still
  computes the real one. Behaviour is identical.
- **More than one fleet at once.** Already filed as its own package, and it is more than a
  root: a single fleet state, a fixed socket name the CLI dials, one live database, and
  pane configuration directories keyed by bare pane name. The layout value is the shape that
  package will need to multiply, which is a reason to prefer it here and not a reason to
  build for it now.
- **The proposal ledger, and anything downstream of a retro.** Filed under the
  self-improvement arc and deliberately built after retros produce content.
- **Changing the evaluator's trigger.** It still wakes only on a handoff, and it still gets
  no button. The sequencing is the design: the answer key does not exist on disk until that
  moment. The Critic is the operator-triggered pane; the evaluator is not.
- **Giving the Critic an answer key, or any judgement of the code.** It reads the archive
  and nothing else. Judging whether the work is correct is peer review's job among workers,
  and the evaluator's job in an experiment.
- **Any change to the message path.** No new module sits between a send and a pty; no
  queue, no gate, no transform. Placement is reached the way a spawn is reached, which is
  not a road any message travels.
- **Any change to the verb list, the wire contract, or the hub.** The arc adds a pane
  identity, which is a new variant of an existing frozen type, not a new seam. Nine verbs
  stay nine.
- **Changes to either existing brief.** The orchestrator and worker briefs are untouched, so
  the shared prompt budget is untouched.
- **A frontend test runner.** Named above, and left to its own work.
- **Unifying the run archive's own entry points.** The architecture review found the archive
  functions each take their root as a first argument and the four commands over them are
  pass-throughs. Real, and a separate spine — the evaluator and the Critic both read from
  it, so it should not move in the same package that changes how panes are placed.

---

## Tickets

Nine tracer-bullet slices, each one a complete path rather than a layer. Working copies with
acceptance criteria live under `.scratch/placement-and-the-critic/issues/`; this table is the
map and the blocking edges.

**Placement slices by pane kind, not by layer.** Its blast radius is about nine files and every
public function in the process-shaping module has exactly one production caller, so it is not a
wide refactor needing expand–contract. The placement function takes one pane kind at a time
while the old sequence keeps the rest, and the suite is green at every step.

| # | Ticket | Blocked by | What it delivers |
|---|---|---|---|
| 01 | Spike — critique an archived run by hand | — | Whether a judge with no answer key finds anything worth reading, plus the prompt that becomes the Critic's brief. Zero code. |
| 02 | Layout and Host, with orch placed through them | — | The orchestrator comes up through the new seam, against a layout it is handed. First tests run in a scratch directory. |
| 03 | Workers placed | 02 | The worktree, the Fence, the toolchain and the gauge move in — and with them the four tests that do not exist today, including re-seeding after a target switch. |
| 04 | The evaluator placed, and the guardrail test drops its copy | 03 | The seam covers every pane kind. The veil is tested for the first time, and the hand-copied ritual in the guardrail test is deleted. |
| 05 | The old sequence deleted | 04 | The interface shrink: the long match goes, and the seeding and command-building functions become internals nothing can call out of order. |
| 06 | The target is fixed once a pane exists | — | The rule stops being enforced by a hidden control and becomes a property of the backend. Behaviour change, own entry. |
| 07 | The evaluator becomes a view | — | The second window and its teardown hazard go; the terminal survives switching away. Also fixes the view-restore list, already wrong today. |
| 08 | The Critic critiques the live run | 01, 05, 07 | Press Start and a real terminal reads the run in progress and reports what the fleet did, every claim cited. It cannot message the fleet. |
| 09 | The Critic critiques an archived run | 08 | Critique any past run from History — the case that makes the feature useful for closed work, and the one that tunes the brief. |

```
01 ───────────────────────────────┐
02 ─→ 03 ─→ 04 ─→ 05 ─────────────┼─→ 08 ─→ 09
07 ───────────────────────────────┘
06  (independent)
```

**On the edges.** Ticket 07 is deliberately *not* blocked by placement: the view move is the
window-and-root layer, which placement never touches, so there is no logical gate. It does edit
the same region as 04 and 05, so running them concurrently risks a conflict — a scheduling
consideration, not a dependency. Ticket 06 is independent of everything. Ticket 01 needs no code
and can run today, before anything else is picked up.

**Package mapping.** 02–05 are WP-21. 07 is WP-22. 01, 08 and 09 are WP-23. 06 stands alone and
belongs to no package — it is the behaviour-change half carried out of D14.

---

## Further Notes

**On what this refactor does and does not buy.** It finds approximately zero bugs today.
The sequence works for the paths that have been exercised by hand. Its value is entirely in
what the code does next, and rests on two facts: the space is combinatorial rather than
linear — three pane kinds against a configured or fallback target, against a target that is
or is not a repository, against a machine with or without a toolchain, against a first run
or one after a target switch — and the file holding it is the most-changed source file in
the repository, with the two packages after this arc landing squarely on it. If the spawn
path were finished changing, most of the value would evaporate. It is not.

**On the naming discipline.** Three of the fifteen decisions were naming decisions, and one
of them caught a collision that would have shipped: a pane called the Reviewer sitting
beside two briefs that teach workers to ask their reviewer to look. Grepping candidate names
against the prompts, not only against the source, is what caught it, because the collision
was in prose the models read rather than in code.

**On the two panes staying two.** The temptation to merge them will recur, because they
share a placement row: same narrow roots, same run-reading, same absence from every
enumeration. The reason to resist is that one is an instrument and one is a tool. The
grader must be frozen and hidden or a generation comparison means nothing; the Critic must
be editable and available or it is not useful. Sharing the mechanism is right; sharing the
identity is not.

**Sequence, including the spike.**

```
  spike (zero code, any time) ──► a measurement note + a proven prompt
                                              │
  WP-21  placement · layout · host            │
         pure refactor, no behaviour change    │
    │                                          │
    ▼                                          │
  WP-22  the evaluator becomes a view          │
         supersedes part of D-066              │
    │                                          │
    ▼                                          ▼
  WP-23  the Critic ◄────────────────────────────┘
```

**Decisions this arc will need to record.** WP-22 supersedes the window half of D-066 and
must say so. WP-23 introduces a pane identity and a brief file, both Tier 2. The
target-freeze fix carried out of D14 is a behaviour change and takes an entry of its own.
WP-21 is internal module structure, which `building.md` places in Tier 3 and which needs no
entry — but the layout and host values landing on the fleet's live state are worth a line
regardless, because the next package to need them will look for one.

**Contention.** WP-23 changes the pane identity type, which is a frozen serde contract
shared by the CLI argument, the database payload, the event field and the TypeScript
mirror. It moves in one commit, and nothing else touching that type should be in flight at
the same time. Neither prompt file is edited by any package in this arc, so the
verb-and-brief contention rule does not apply.
