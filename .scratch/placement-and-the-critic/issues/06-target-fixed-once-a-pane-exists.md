# 06: The target is fixed once a pane exists

**What to build:** Setting or picking the target after a pane is already running is refused, with a sentence the operator can act on, instead of being prevented only by the interface hiding an input.

The rule already exists and is already written down — the target is safe to change at the start gate, and once panes are running the interface hides the control. But the backend command is exposed unconditionally, so the rule lives in the interface rather than in the thing that owns the state. Half a fleet in one repository and half in another is incoherent, which is the same reasoning that makes the target resolved once at bootstrap rather than re-read.

This is a behaviour change — a command that used to succeed now fails in one case — so it is its own ticket and its own decision entry, kept out of the pure-refactor tickets on purpose.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Setting the target before any pane exists works exactly as it does today
- [ ] Setting the target once any pane is running is refused, and the message says the target is fixed for a running fleet and how to change it (stop the fleet)
- [ ] The same rule applies to both the picker and the typed path — neither is a way around the other
- [ ] A test covers both sides: accepted before the first spawn, refused after
- [ ] The interface still hides the control when panes are running; it is now belt and braces rather than the only guard
- [ ] Recorded as a decision entry, since it changes what a command does
