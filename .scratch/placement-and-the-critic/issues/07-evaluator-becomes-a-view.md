# 07: The evaluator becomes a view

**What to build:** The evaluator moves out of its own operating-system window and into the main window's rail, beside Terminals, Messages, Tasks, Activity and History. Selecting it shows its terminal; switching away and back finds it exactly as it was left.

The second window's stated justification is that mounting the main application root twice would give the app two webviews racing for one fleet. That argues for the second window having its own root, not for the second window existing — a view inside the single root has no such race. The rule that terminals are never unmounted is already honoured by every existing view through hiding rather than unmounting, so a view holding the evaluator's terminal satisfies it by the same mechanism.

What goes away with the window: the branch that decides which root to mount, the interception that turns a close into a hide so the terminal buffer survives, the per-window capability scoping, and the window-event handler that once tore down all six ptys because it could not tell one window from another.

The tab is present only in dev mode, and it carries no start control — the evaluator wakes when the orchestrator hands off, and the absence of a button is the design showing through, not a gap.

Fix the view-restore list while here. It already omits one existing view, so choosing that view and relaunching silently lands somewhere else; this ticket adds another view to get wrong.

**Blocked by:** None (can start immediately). Note: it touches the same area as tickets 04 and 05, so running it concurrently with them risks a conflict even though nothing gates it.

**Status:** ready-for-agent

- [ ] The evaluator's terminal appears as a view in the rail, and no second window is created
- [ ] Its terminal and scrollback survive switching to another view and back
- [ ] The view is absent outside dev mode
- [ ] Before a handoff the view explains that the evaluator wakes on one, and offers no control to start it
- [ ] The window-label branch, the close interception and the per-window capability scoping are removed
- [ ] Closing the application is unambiguous; no window's close can reap more than intended
- [ ] The view-restore list names every view the rail declares, including the one it omits today
- [ ] A source-reading tripwire asserts those two lists name the same views, in the style of the existing tripwires
- [ ] The stale "review" label on the evaluator's terminal is retired — it collides with peer review, which is what the briefs teach
- [ ] Recorded as a decision entry superseding the window half of the decision that chose one
