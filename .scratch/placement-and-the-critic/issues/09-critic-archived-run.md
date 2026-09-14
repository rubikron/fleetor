# 09: The Critic critiques an archived run

**What to build:** Pick any past run in History and critique it. This is the case that makes the Critic useful for work already finished, and the one an operator would reach for while tuning the brief — critiquing the same archived run repeatedly is how the prose gets better, and it costs nothing to repeat.

Nothing about how the Critic reads changes. It already reads a snapshot; pointing it at a run that is over rather than one in progress is the same operation against a directory that has stopped moving. There is no live orchestrator to contaminate, so this path carries none of the sequencing consequences the evaluator's trigger does.

**Blocked by:** 08 (The Critic critiques the live run).

**Status:** ready-for-agent

- [ ] Each row in History offers a way to critique that run
- [ ] Choosing it opens the Critic view against that run and names which run is being read
- [ ] The Critic reads the archived run's events and transcripts, and nothing from the live run
- [ ] Critiquing a past run cannot send to, alter, or otherwise touch the live fleet
- [ ] Critiquing the same archived run twice works, so the brief can be tuned against fixed input
- [ ] A test places a Critic against a named archived run and asserts its working directory and its guardrail roots
