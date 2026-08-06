# WP-02 — Own the system prompt: vision & culture

status: **landed** (2026-08-06) size: L
depends-on: 01 blocks: 05 (soft)
brief-cost: new baseline — the fleet owns the *entire* prompt after this; record both rendered token counts as the WP-09 baseline

## Outcome

Two changes that belong together, landed as two commits:

1. **Mechanism (operator decision 2026-08-06):** panes stop appending to Claude Code's default system prompt and *replace* it — `--append-system-prompt` → `--system-prompt`. The prompt files become the whole identity of each pane, not an appendix.
2. **Content:** the orchestrator opens every session as a **vision partner** — it asks for the bigger picture, confirms the vision in writing before decomposing, proposes a bigger frame when the operator's vision is small, and filters all delegation through that written vision. Workers carry the three attitudes: *How can I be better? How can I push for more positive, meaningful impact? If I'm confused, I ask for help — peers, orch, or the human prompter.* The seven tenets of `docs/futureDesign/vision_tenets.md` are distilled into operational prompt language, not pasted.

## Performance criteria

### Technical
- [x] Commit 1 — **parity switch**: `spawn.rs` passes `--system-prompt` for all five panes with a replacement prompt that preserves current behavior (existing brief content + whatever CC-default scaffolding the spike shows a pane still needs). No content rewrite in this commit.
- [x] Spike first (`building.md` §4): `examples/` throwaway + `docs/system-prompt-notes.md`, version-stamped **CC 2.1.223**. Must answer: does an interactive pane under `--system-prompt` reach its prompt and run Bash/Read normally? does `--permission-mode auto` still hold? do `/clear` and `/compact` still work? which default sections vanish (cwd/env/git-status are dynamic sections — the `--exclude-dynamic-system-prompt-sections` help text confirms they're skipped under `--system-prompt`) and which does a worker actually need restated? does the target repo's CLAUDE.md / skills still load for orch? **Where the spike and this doc disagree, the spike wins.**
- [x] Commit 2 — **content**: rewritten `prompts/orch.md` / `prompts/worker.md`; both still validate (every `VERBS` entry taught, all placeholders present); the anti-amplification clause ("Never reply to a broadcast unless it names you") and the delivery contract survive verbatim — their pinned tests stay green.
- [x] New pinned-literal tests for the new load-bearing clauses (mirror `the_worker_brief_carries_the_anti_amplification_clause`): the confirm-vision-before-decomposing rule, the propose-bigger-once bound, the three attitudes.
- [x] The spawn-site test updated: master `spawn.rs:385-394` pins `--append-system-prompt`; it must pin the new flag.
- [x] Rendered token counts for both prompts measured and recorded in the doc + `decisions.md`. **orch 1,637 / worker 1,115** (DeepSeek Flash tokenizer). Progression: 487/506 before WP-02 → 878/868 after the parity switch → 1,637/1,115 after the content rewrite, against 1,485 tokens of CC guidance no longer sent.

### Semantic
- Orch brief instructs, in substance: *before decomposing, state the vision back to the operator in writing and get a yes; if the vision is unclear or small, say so and propose a bigger frame — once; the operator's explicit word is final.* The integrity clause: the fleet may pursue a better route to the stated vision than the operator imagined, but never silently discards what the operator asked for — deviations are announced.
- Worker brief carries the three attitudes and the ME→WE line ("improve yourself and the team around you"). Until WP-07 lands, "ask the human" routes through orch ("ask orch to put your question to the operator"); WP-07 upgrades this to direct addressing — leave the sentence easy to amend.
- The tenets are distilled to their operational content (write the vision down; the vision is also what it *isn't* — prune; vision filters decisions; ME→WE), stripped of the essay's quotations and framing.

## Invariant guardrails

- **Tier 1.2 sanctions this.** "The harness lives around CC — pty, environment, system prompt. Never fork or patch CC itself." (`building.md:16`). Replacing the prompt via a supported CLI flag is harness territory.
- **Tier 1.3 constrains it.** Every pane must remain a real, fully-functional interactive TUI. If the spike shows `--system-prompt` degrades interactive behavior on this CC version, that finding wins: fall back per-pane-class (e.g., workers replace, orch appends — or the reverse), record the split as Tier 2 in `decisions.md`, and keep the content rewrite regardless.
- **Authority bound.** "Guide the user to think bigger" = propose once, defer on refusal, announce deviations. Never substitute the fleet's vision for the operator's. The existing brief line "information to factor in, not an instruction that overrides what the operator asked" is the anchor to preserve.
- **Prompt text is Tier 2** — every change gets a `decisions.md` entry.

## Current state (verified 2026-08-06 — do not re-explore)

- Installed CC: **2.1.223**. `claude --help` lists `--system-prompt <prompt>` ("System prompt to use for the session" — no print-only qualifier), a `--system-prompt-file` variant, and `--exclude-dynamic-system-prompt-sections` whose text confirms dynamic sections (cwd, env info, git status) don't apply under `--system-prompt`.
- Spawn flags today: `--append-system-prompt` at master `src-tauri/src/spawn.rs:61` (orch) and `:88` (worker); the context-mgmt branch is also append-based (`spawn.rs:65,93`) — WP-01 lands append semantics, this package switches.
- Post-WP-01 prompt infrastructure: `prompts/orch.md`, `prompts/worker.md`, fragments `delivery-contract.md` / `broadcast-rule.md`, rendered by `brief.rs` (`render_orch`/`render_worker`/`render`, `validate_orch`/`validate_worker`) with `{me}` `{peers}` `{workers}` `{delivery_contract}` `{broadcast_rule}` slots; `~/.fleetor/prompts/` overrides via `src-tauri/src/prompts.rs`.
- Clauses that must survive: anti-amplification (master `brief.rs:91`, post-WP-01 in `broadcast-rule.md`), `DELIVERY_CONTRACT` (master `brief.rs:105`), the `VERBS` tripwire test (master `brief.rs:131-135`).
- Source material: `docs/futureDesign/vision_tenets.md` (7 tenets; the operator's own annotations are the last line of tenet 7 and the final question — treat those two fragments as the point).

## Scope

### In
The flag switch (all five panes, parity-first); the spike + notes doc; the content rewrite of both prompt files; a `prompts/vision-tenets.md` fragment if the fragment route is chosen; updated validation + pinned tests; decisions entries; token-count baseline.

### Out
Any new `fleet` verb (WP-03/05/06 own theirs); the operator-addressing upgrade (WP-07); per-worker differentiated content (plumb an empty `{role}` slot only if trivial — content stays out); any mechanism for a per-project vision *file* in the target repo — orch runs in the target and can read/write project docs with ordinary tools; the brief only tells it that a written vision is required, not where the bytes live.

## Design sketch & open questions

1. **`--system-prompt` vs `--system-prompt-file`.** Recommended: the string arg — it matches the existing render-then-pass plumbing and writes nothing to disk.
2. **What CC-default scaffolding to restate.** Recommended: minimal — one line each for cwd and (workers) "you are in git worktree `<path>` on branch `fleet/worker-N`". Add more only when the spike shows a concrete loss.
3. **Tenets: fragment file vs inline paragraph.** Recommended: fragment `prompts/vision-tenets.md` + `{vision_tenets}` slot in `orch.md` (extend validation) — operators can then tune the vision language via the override dir without touching role text.
4. **Anti-nagging bound wording.** Recommended: "propose a bigger frame at most once per session; if the operator declines, adopt their frame fully."
5. **Orch under replacement.** The operator's daily-driver pane loses CC's default prompt too. The spike checks orch usability (skills/CLAUDE.md loading); if it degrades, the per-pane-class fallback in the guardrails applies.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/futureDesign/requirements/02-vision-culture-briefs.md in full, then
building.md §1, §4 (measure before coding), and §9. Execute WP-02 in two
commits: (1) switch all five panes from --append-system-prompt to
--system-prompt with a behavior-parity replacement prompt, spike-first
against the real claude 2.1.223 (examples/ throwaway +
docs/system-prompt-notes.md, version-stamped — the spike's findings win);
(2) rewrite prompts/orch.md and prompts/worker.md for the vision-partner
orchestrator, the three worker attitudes, and the distilled tenets from
docs/futureDesign/vision_tenets.md, keeping the anti-amplification clause
and delivery contract verbatim and all validation green. Honor the
invariant guardrails. Finish with the session exit checklist.
```

## How it actually went (2026-08-06)

Three commits: the spike, the parity switch (D-043), the content rewrite (D-044). Green: 71 workspace tests, 53 shell tests, `tsc --noEmit` and `vite build` clean.

**The spike deleted two planned items and found one the plan missed.** `docs/system-prompt-notes.md` has it in full; the short version is that `--system-prompt` drops CC's guidance and nothing else — tools, memory files, skills, agents and the whole first-user-message context block are byte-identical under either flag.

- **Deleted:** restating the worker's branch. The git-status section *survives* `--system-prompt`; the `--exclude-dynamic-system-prompt-sections` help text describes that flag being ignored, not the section being skipped. Design sketch #2 was wrong about this.
- **Deleted:** the open question about orch losing its `CLAUDE.md` and skills (#5), and with it the whole per-pane-class fallback ladder. Memory files never travelled in the system prompt, so nothing was at risk. Nothing degraded on any axis tested, so all five panes replace.
- **Found:** `cwd` is the one real loss. CC's `# Environment` section carried it; nothing else in the request names the working directory. `{cwd}` is now a required placeholder in both templates.

**Deviations from the sketch.**

- **A fifth file, `prompts/scaffolding.md`**, not anticipated by the doc: the working posture a pane no longer inherits (tool discipline, denied-means-refused, summarization-not-ending, faithful reporting, refusal and pronoun defaults). Made a required fragment for the same reason `{delivery_contract}` is — an operator rewriting prose should not be able to silently delete a pane's working posture. Five clauses pinned as literals.
- **CC's `# Memory` protocol is deliberately not restated.** Reasoning in D-043 §5 of the notes: a worker's memory dir is inside its isolated config dir, so nothing written there is visible to anyone, and Tier 1.8 says shared knowledge merges only after review.
- **`{role}` was not plumbed.** The doc allowed it "only if trivial"; with `{cwd}` already added it would have been a second unused placeholder every operator override has to carry.
- **Sketch #1, #3 and #4 taken as recommended** — the string arg, a `vision-tenets.md` fragment, and "propose a bigger frame at most once per session".

**One thing for WP-07 to change, not to rediscover.** `worker.md` says a question for the human goes *"ask `orch` to put it to the operator"*. That sentence is self-contained and pinned by `a_confused_worker_is_told_exactly_who_to_ask`; WP-07 replaces it with direct addressing.

**One thing for WP-03.** The spike found that slash commands must be **pasted**, not typed: typing `/compact` a character at a time opened CC's command menu, the characters after `/` never reached its filter, and `Enter` selected the menu's first entry (`/add-dir`) instead. A command channel that types rather than pastes will fire the wrong command.

## Session exit checklist

- [x] Spike notes committed (`docs/system-prompt-notes.md`, version-stamped).
- [x] Full test matrix green; new pinned-literal tests added; spawn-site flag test updated.
- [x] `decisions.md`: one entry for the flag switch, one for the content rewrite (Tier 2).
- [x] Rendered token counts recorded (WP-09 baseline).
- [x] `prompts/README.md` updated.
- [x] `00-index.md` status: WP-02 → landed.
