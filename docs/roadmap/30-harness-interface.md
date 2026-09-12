# WP-30 — `harness.rs` becomes the interface: one file per harness

status: not-started size: M
depends-on: 25 (the seam, landed) blocks: 24 (register cursor)
brief-cost: 0 — no prompt changes. This moves code and re-points two mechanisms.

**The spec lives in [issue #52](https://github.com/rubikron/fleetor/issues/52), not here.** This doc
is the roadmap's pointer to it, in the shape WP-24 and WP-25 already use. The issue carries the
problem statement, 42 user stories, the implementation and testing decisions, and the
out-of-scope list. The decision trail — every option considered, every rejected alternative,
and what would reverse each choice — is `decisions.md` **D-090**.

## Outcome

WP-25 built the `Harness` seam and it holds. What it does not yet have is an honest file
layout: `harness.rs` is both the interface and one vendor's answers, and both vendors'
implementations live in shared modules under generic names. This makes the layout match the
trait — one file per harness, owning every line of its own behaviour — so that registering a
third TUI is writing one file and adding one line, with conformance and the re-hardcoding
tripwire both covering it the moment it registers.

Nothing an operator can see changes. What changes is the cost of the next harness, and that
cost was measured once already: registering codex forced six checkpoint reshapes (C57). The
three gaps this closes are the ones that were worked around rather than fixed during that
registration.

## Scope

**In:** the extraction of Claude Code's spec, impl, diagnostic and unit tests into their own
module; the four vendor bodies currently in `spawn.rs` and `context_gauge.rs` moving to the
file that owns them; the JSON-document seeder splitting into a neutral support seam plus
Claude Code's own claim; the machine probe becoming a `Harness` method taking facts as an
argument; the gate iterating the registry; the literals tripwire iterating the registry with
per-harness needles.

**Out:** registering a third harness (that is WP-24); data-driven or runtime registration in
any form; auto-discovery in place of the `REGISTERED` array; changing which harness the two
judges run (C15) or the unpicked fleet's defaults (C78); reshaping `HarnessReadiness`; adding
or removing a checkpoint; any frontend change (verified unnecessary); the standalone
harness-authoring guide (R11, still unwritten and still worth writing); any new vendor
measurement, so no live spend.

## Invariant guardrails

- **`placement` reads nothing from the process** — the module's one rule, asserted by
  `tests/placement_reads_nothing.rs`. This work *tightens* it: codex's probe currently
  inherits the application's environment behind an `Installation::default()`, and folding the
  probe onto the trait with an explicit facts argument closes that hole.
- **Tier 1.7** — nothing here widens what a pane may do. No guardrail root moves; the
  guardrail's roots stay placement's own computation from the pane's cwd.
- **The compiler stays the checklist** — `HarnessSpec` keeps its absent `Default` and the
  trait keeps its undefaulted methods (R10). This spec relaxes no obligation on a harness.
- **Tier 1.4/1.5** — the message path is untouched. No gate, no queue, no transform is added
  between `fleet send` and a pty.

## Landing order

Three commits, in order — the reason for the split is in the issue:

1. The file move. Pure extraction, no behaviour change.
2. The probe onto the trait, plus normalizing codex's probe to take its facts as arguments.
3. Needles onto the vendor files; the tripwire iterates the registry.

## Session prompt

```
Read docs/roadmap/30-harness-interface.md, then GitHub issue #52 in full — the spec lives
there. Read decisions.md D-090 for the decision trail, and building.md §1 and §9.

Land the three commits in the order the spec names. Do not start commit 2 until commit 1 is
green with no conformance test edited to accommodate a move: that suite passing unchanged is
the proof the extraction preserved behaviour.

Commit 2 reverses a decision recorded in placement's own source, at Host::discover_for_the_gate.
Read that argument before implementing it. It is not being overruled casually — the reason it
loses is that its vendor-fact/machine-fact distinction was already broken by
Harness::read_operator_login, which is on the trait today and reads the macOS keychain.

Commit 3 may surface genuine hard-coding when the tripwire starts looking at codex. That is a
finding: report it, do not paper over it. It was checked at spec time and should land green.

No live spend is needed. If you find yourself wanting to measure a vendor, stop — that is out
of scope and belongs to whichever package needs the measurement.
```

## Session exit checklist

- [ ] `decisions.md` entry for anything that moved beyond D-090 (cite the D-number in the commit subject).
- [ ] The conformance suite is green over both harnesses with no test edited to accommodate a move.
- [ ] `tests/harness_literals.rs` iterates `registered()`, and each harness carries its own needles.
- [ ] `tests/placement_reads_nothing.rs` still passes, and codex's probe no longer reads the process.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.
