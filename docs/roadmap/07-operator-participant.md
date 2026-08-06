# WP-07 — Operator as participant

status: **landed** (D-051) size: M
depends-on: 01 blocks: —
brief-cost: budgeted +~60 tokens; **spent +126 orch / +135 worker** (measured, D-051 / Q-4 — the smallest overrun of the four, and the control case for "a verb costs what its rules cost": this package added no verb)

## Outcome

The human joins the record. The operator can send a **logged, framed** fleet message from the UI — to one pane or broadcast — arriving as `[fleet · operator] …` through the unchanged delivery path. And agents can address the human: `fleet send operator "<question>"` is recorded and surfaced in a visible operator inbox. This closes the autonomy review's gap ("the one participant with real intent is the one the log cannot see") and makes the third attitude real: *if I'm confused, I can ask for help — even the human prompter.*

## Performance criteria

### Technical
- [ ] New identity `PaneId::Operator`, serialized `"operator"`. Verified 2026-08-06: `"operator"` currently **fails** to parse (`pane.rs:80-91` — not in the orch alias list, no `worker`/`w` prefix match, not a number), so the name is free; add it as an exact-match arm. `"o"` keeps meaning orch.
- [ ] Exhaustive-match fallout handled deliberately: hub routing, delivery, roster, spawn (operator is **never spawnable/killable**), UI types.
- [ ] Operator → pane: UI `invoke` → hub → the **unchanged** deliver path (framed, pasted, `accepted` semantics identical to any pane's send).
- [ ] Pane → operator: there is **no pty target**, so it gets a distinct outcome word — **`recorded`** — rendered distinctly. `accepted`'s definition ("bytes reached a live pty") is not diluted (Tier 1.5).
- [ ] `fleet reply` works both directions — `last_inbound_from` is keyed by `PaneId` already; an operator send becomes the target pane's reply target; a pane→operator send does the same for the operator's composer.
- [ ] Roster lists the operator with a state label that is not a pane state (recommended: `present`) — never a faked `Live`.
- [ ] Broadcast from the operator uses the `→ all` framing; the broadcast-rule clause binds replies to it exactly as for pane broadcasts.

### Semantic
- Worker prompt drops/amends "the human is watching orch, not you": the operator is a reachable participant whose messages carry **final authority** (consistent with WP-02's authority clause). Asking the operator is sanctioned when confusion is real — that is the culture, not an escape hatch from thinking.
- The composer lives in the Messages view. Typing into a terminal remains what it is — raw keystrokes, unlogged. This package adds a logged channel; it does **not** log keystrokes.

## Invariant guardrails

- **Tier 1.5 vocabulary discipline:** three words now exist — `accepted` (bytes reached a live pty), `recorded` (entered the log; no pty exists), and the never-rendered `delivered`. They never blur.
- **The message path is untouched** for pane↔pane traffic; operator→pane rides the existing path unmodified.
- **No attention machinery.** No notifications, no unread nagging beyond the existing gold-badge idiom — the Open Loops alert was cut upstream for rebuilding supervision; the inbox is a surface, not a system.

## Current state (verified 2026-08-06 — do not re-explore)

- `PaneId` + `FromStr`: `crates/fleetor-core/src/pane.rs:26,73-92`; serde as bare string `:94-105`; `PaneEntry` `:145`. Parse test to extend: `parses_every_spelling_a_model_is_likely_to_type`.
- Hub: `hub.rs:136` dispatch, `:148` self-send guard (operator self-send should refuse identically), `:227` deliver, `:236` `log_message` / `last_inbound_from`, broadcast targets from the app's live roster `:159`.
- UI seam: `ui/src/fleet/api.ts` (9 invoke wrappers — add one for the composer), `ui/src/fleet/types.ts` (hand-maintained TS mirrors), `MessageFeed.tsx` (render + inbox), `src-tauri/src/lib.rs` (invoke command registration).
- Framing: `message.rs` `frame_for_pane`/`frame_broadcast_for_pane` — `[fleet · operator]` falls out of `Display` once the variant exists.
- DB: old rows never contain `"operator"` (new name) — no migration.

## Scope

### In
`PaneId::Operator` + parse/serde + exhaustive fallout; composer UI (target picker: pane or all); operator-inbox rendering in Messages; `recorded` outcome word end-to-end (wire → event → TS → render); prompt amendment; tests (parse, self-send refusal, operator broadcast framing, `recorded` never `accepted`).

### Out
Notifications/attention management; multiple humans/auth; logging terminal keystrokes; any spawn/kill surface for the operator identity.

## Design sketch & open questions

1. **Where `recorded` lives.** Recommended: `OpResult::Delivered { accepted, detail }` stays untouched for panes; pane→operator returns a distinct result (or `detail: "recorded"` with `accepted: true` is **not** acceptable — prefer a new `OpResult::Recorded { msg_id }`). The wire is Tier 2; log the change. — **Settled differently (D-051):** WP-05 had already landed `OpResult::Recorded { task_id }`, and two variants under one serde tag cannot coexist. There is **one** variant, `Recorded { record_id }`, meaning *entered the log, no pty written to* — a task claim and a message to the human are the same event class. `FleetEvent::Message` gained no field: the row carries `accepted: false, detail: None` (literally true when there is no pty) and every renderer derives the word from `PaneId::has_pty(to)`.
2. **Composer affordance.** Recommended: single-line input + target select at the bottom of the Messages view, matching the warm-theme idiom; ⌘Enter sends. — **Taken as recommended**, with the target select resolving to the fleet's three existing verbs (a pane name → `send`, `all` → `broadcast`, `reply` → `reply`) and defaulting to the last pane that wrote, until the operator picks one themselves.
3. **Does orch get told?** Recommended: yes — one prompt line telling orch the operator can now speak in-band and workers may address them directly. — **Taken**, and it needed the second half explicitly ("that is sanctioned, not a worker going around you"), or an orchestrator seeing a worker↔operator exchange in its own input has every reason to police it.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/roadmap/07-operator-participant.md in full, then
building.md §1 and §9. Execute WP-07: add PaneId::Operator ("operator" —
verified free in FromStr), the UI composer that sends logged framed
messages through the unchanged deliver path, the operator inbox in the
Messages view, and the distinct `recorded` outcome for pane→operator sends
(never `accepted` — no pty exists). Operator is never spawnable; roster
shows a non-pane state label. Amend the worker prompt: the operator is
reachable and their word is final. Prompts + tests move with the change.
Finish with the session exit checklist.
```

## Session exit checklist

- [x] Full test matrix green; vocabulary tests pin `recorded` vs `accepted`.
- [x] `decisions.md` entry (new identity, wire result variant) — D-051.
- [x] Prompt amendment landed with validation green (no new verb).
- [x] `00-index.md` status updated.
