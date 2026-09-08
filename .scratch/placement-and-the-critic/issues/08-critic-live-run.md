# 08: The Critic critiques the live run

**What to build:** A second run-reading pane, for ordinary work. Open the Critic view, press Start, and a real terminal wakes, reads the run in progress, and reports what the fleet actually did — with a timestamp, file and line from the archive behind every finding.

It exists whether or not dev mode is on, because it is a product feature rather than an instrument of an experiment. Its brief ships in the prompts directory where the operator can rewrite it, unlike the evaluator's, which is compiled in from a separate repository precisely so the fleet can neither find nor edit it.

**It judges the run, not the work.** It has no answer key and never will, so it is told to report only what the archive proves: idle time, blocks marked done whose check never ran, blocks posted with no performance criteria, two workers editing one file, messages that got no reply. A claim it cannot point at is not a finding.

**It cannot message the fleet.** Findings reach the operator through its own view and the Activity feed. Anything worth acting on the operator forwards through the composer they already have, which keeps the human as the only writer and costs the orchestrator's brief nothing.

It is a real, typeable terminal, so the operator can argue with a finding or ask it to look again.

From the design prototype — the placement input, which is where the run being critiqued travels:

```
enum PaneSpec { Orch, Worker(slot), Evaluator, Critic { run: RunSource } }
enum RunSource { Live, Archived(id) }
```

**Blocked by:** 01 (the spike, which produces the brief and the reason to build this), 05 (the old sequence deleted — the Critic is one placement variant only once placement owns every pane), 07 (the evaluator becomes a view — the Critic uses the same view pattern).

**Status:** ready-for-agent

- [ ] A Critic view sits in the rail, present with dev mode off
- [ ] Pressing Start wakes a real terminal that reads the live run and reports findings
- [ ] Its brief is a file in the prompts directory, editable by the operator, and is the prompt the spike converged on
- [ ] Every finding carries a citation into the archive; the brief refuses claims that cannot be pointed at
- [ ] The brief commits it to the run and forbids judging whether the code is correct
- [ ] It has no route to the fleet: `fleet send` from inside it does not reach a pane
- [ ] Its write-guardrail roots are its own working directory alone, like the evaluator's
- [ ] It appears in no roster listing, is not a leg of a broadcast, and is in no brief's peer list
- [ ] Neither existing brief is edited, and the verb list is unchanged
- [ ] The tripwires asserting the veil still match the evaluator only and do not begin matching the Critic, which is openly named
- [ ] In dev mode both the Critic and the evaluator views are available, and the rail makes clear they answer different questions
- [ ] Recorded as a decision entry: a new pane identity and a new brief file
