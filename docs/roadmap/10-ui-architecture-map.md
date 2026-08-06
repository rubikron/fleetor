# WP-10 — The `ui/` architecture map

status: not-started size: S/M
depends-on: — blocks: —
brief-cost: 0 (documentation only; no prompt text moves)

## Outcome

The one part of the system with no as-built doc gets one: `docs/ui-map.md`, doing for the 33 TypeScript files what `fleet-comms-map.md` does for the message path. A session changing the frontend should start from the map, not from explore agents — which is the standard the rest of the doc system now meets (D-057).

## Performance criteria

### Technical

- [ ] `docs/ui-map.md` exists, is indexed in `docs/README.md`, and every file under `ui/src/` appears in it.
- [ ] It documents, with file anchors: the component tree and the six views (`fleet | messages | tasks | activity | settings`); how `useFleet` replays `fleet://event` into state; how pty output reaches xterm via the per-pane channels; the hooks (`useContextGauge`, theme, zoom, persisted nav).
- [ ] It carries a "reading the code" ordered walk, in the comms-map §8 style.
- [ ] No code changes. `npx tsc --noEmit && npx vite build` untouched and clean.

### Semantic

- [ ] The two deliberate duplications are explained *as decisions*, not discovered as smells: `ui/src/fleet/board.ts` mirrors the Rust task fold on purpose (D-047), and `ui/src/fleet/types.ts` mirrors `FleetEvent` — five variants, unknown ones silently dropped, so an event change moves both in one commit.
- [ ] The `.is-hidden` rule (terminals stay mounted, never conditionally rendered — building.md §7.5, reason in D-047) is stated where a component author will actually see it.

## Invariant guardrails

None brushed — this package writes one markdown file. The temptation to "fix" the board.ts duplication or the types.ts mirror while mapping them is the tripwire: both are recorded decisions (D-047); mapping is not relitigating.

## Current state (verified 2026-08-06 — directory level only; the file-level map is this package's deliverable)

`ui/src/` holds 33 TS files: `components/` (12), `fleet/` (6, including `board.ts`, `inbox.ts`, `types.ts`, `useFleet`), `ui/` (7 hooks), `lib/`, `dev/`. `building.md` §7 covers visual tokens and layout rules, not code structure. No README exists inside `ui/`.

## Scope

**In:** one as-built doc; a `docs/README.md` index row.

**Out:** any code change, any refactor, any opinion that changes what the code does. If mapping surfaces a real defect, file it — a D-entry question or a new roadmap package — don't fix it here.

## Design sketch & open questions

Follow `fleet-comms-map.md`'s shape: the path (a user keystroke / an incoming event) end to end, then per-area sections, then the failure table if one earns its place, then the reading order. Open question with a default: whether the doc lives at `docs/ui-map.md` (recommended — as-built docs live flat in `docs/`) or as `ui/README.md` (rejected default: the index lives in `docs/`, and splitting homes is what D-057 undid).

## Session prompt

```
Read docs/roadmap/10-ui-architecture-map.md in full, then building.md §1 and §7,
then docs/fleet-comms-map.md (as the model for the doc you are writing). Explore
ui/src/ thoroughly — this is the one package where exploration is the work — and
write docs/ui-map.md meeting the performance criteria. No code changes. Exit per
the checklist: index row in docs/README.md, this doc's status → landed with a
"How it landed" note, 00-index status column updated as the last act.
```

## Session exit checklist

- [ ] `docs/ui-map.md` committed; `docs/README.md` row added.
- [ ] No non-doc file changed; `npx tsc --noEmit && npx vite build` clean.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.
