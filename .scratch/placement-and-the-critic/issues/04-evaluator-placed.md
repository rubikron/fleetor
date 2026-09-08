# 04: The evaluator placed, and the guardrail test drops its copy

**What to build:** The evaluator comes up through the placement module, which completes the seam — every pane kind now goes through one interface. Placing an evaluator re-checks dev mode itself, reading the stored flag through the layout it was handed rather than from a process-global call, so the spawn site genuinely owns that property and a caller cannot lie to it. It lays out the run for reading and renders the brief, because an evaluator's working directory *is* a snapshot of a run and it cannot be placed without one.

**The write-guardrail test stops retyping the sequence.** It currently rebuilds the bring-up steps by hand because it cannot call them, and its copy has drifted: it does not carry the evaluator's narrower guardrail roots. It now drives the real placement function and asserts against what production actually writes. That is the change that stops the copy diverging again, because there is no copy.

This ticket also makes an existing doc comment true. It asserts that the handoff watch and the spawn path share one target; they do not, because the watch carries its own copy while the spawn path reads a field the target-setting command mutates. Both read one source. No behaviour changes — the deliberate freeze is ticket 06.

**Blocked by:** 03 (Workers placed).

**Status:** ready-for-agent

- [ ] The evaluator wakes on a handoff in dev mode and reaches its prompt exactly as before
- [ ] A test asserts the evaluator's guardrail roots are its own working directory alone — narrower than any other pane's. This is the veil, and it has never been tested
- [ ] A test asserts that placing an evaluator with dev mode off is refused, with the flag read from a scratch configuration rather than the operator's real one
- [ ] Placement contains no call to the process-global dev-mode reader
- [ ] The write-guardrail test drives placement for its setup; its hand-built copy of the sequence is deleted
- [ ] The write-guardrail test keeps executing the installed hook through a real shell against real tool-call payloads — that seam is unchanged
- [ ] The handoff watch and the spawn path read one target; no behaviour change
- [ ] The source-reading tripwires still pass, and the ones asserting the veil still match the evaluator only
