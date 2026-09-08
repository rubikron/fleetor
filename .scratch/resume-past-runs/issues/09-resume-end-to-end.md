# 09: Resume end-to-end

**What to build:** The operator explicitly Resumes a past run and keeps working: a dialog states plainly they are branching history into a new run **and** ending the current fleet; the live fleet is torn down to the single live slot; every worktree is recreated as-left; the orch resumes in the real target repo; all five panes come back live and unfenced, coordinating; the carried-forward task board and Activity feed are present.

**Blocked by:** 08, 05.

**Status:** ready-for-agent

- [ ] A Resume action on a past run shows a fork dialog naming both effects (a new forked run + ending the running fleet) before anything happens.
- [ ] Confirming tears down the current live fleet first (single live-run slot preserved), then brings up the forked run.
- [ ] Each worker worktree is recreated as-left (07) at the recorded `branch@commit`; the orch resumes in the recorded target-repo path.
- [ ] All five panes resume live with input unfenced; the `fleet task` board and the Activity feed show the carried-forward coordination.
- [ ] Fake-pane seam asserts each seat's resume is invoked with the restored home/config and the restored worktree cwd.

Note: Claude Code worker seats resume live here; Codex seats resume live once 06 has landed, else degrade per 10.
