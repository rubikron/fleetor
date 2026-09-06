//! **The posture tripwire: what FLEETOR writes, read back out of the vendor**
//! (WP-25 #37; C8, C14, C36, C47).
//!
//! Writing `sandbox_mode`, `approval_policy` and
//! `sandbox_workspace_write.network_access` proves nothing about the fence. Measured
//! on `codex-cli 0.153.4`: a key the vendor no longer knows leaves `doctor` emitting
//! a perfectly ordinary report with the override **silently dropped**. So a release
//! that retires one of the three produces a fleet that is configured to be fenced and
//! is not — or, for the network row, one that spawns clean and cannot reach the
//! socket. That is the arc's signature failure class, and its whole signature is
//! that nothing on screen says anything.
//!
//! ## What is asserted here, and what is asserted elsewhere
//!
//! **The behaviour is asserted where it runs.**
//! `src-tauri/src/fleet.rs`'s `a_containment_the_vendor_did_not_resolve_refuses_the_start`
//! runs the real `StartVerdict::for_seats` against a machine a value *describes* and
//! shows the fleet refusing, once per key, with the key named. That is the load-bearing
//! test, and it was verified by mutation: gutting the comparison turns it red.
//!
//! **What this file adds is the half that has no behaviour to run** — the wiring
//! between three static lists that type-check independently and would rot in silence:
//!
//!  - every key the seeder writes has a row that reads it back, and every row names a
//!    key the seeder writes ([`every_containment_key_is_read_back_and_every_read_back_is_a_key`]);
//!  - a harness that verifies anything pins the report shape it verified against
//!    ([`a_harness_that_reads_a_posture_back_pins_the_schema_it_read_it_from`]);
//!  - and the comparison is on the gate's reading and only ever the gate's
//!    ([`the_spawn_paths_own_reading_has_no_posture_to_disagree_about`]).
//!
//! ## Why this is not #35's fifteen checks again
//!
//! #35's source tripwire passed on a screen that could never load, because reading
//! source proves the code *says* the right thing and nothing about whether it runs.
//! Every check below is therefore a call into the real thing — `posture_disagreements`,
//! `registered()`, `Host::discover()` — rather than a `contains()` over a file. The one
//! property that genuinely lives in a static list is asserted by walking that list, not
//! by grepping for it.
//!
//! **What no test here can prove**, said out loud rather than implied: that
//! `restricted`, `enabled` and `Never` are still the words the vendor uses. Those came
//! off a real binary and are recorded in `CODEX_SPEC.posture.verified_as`; a release
//! that renames one is caught by the gate refusing on the operator's own machine,
//! which is exactly what this ticket built. Asserting it in the suite would mean a
//! vendor call, and `tests/vendor_binary_tier.rs` is where vendor calls live.

use fleetor_shell::placement::harness::{
    self, AccountShape, HarnessReadiness, HarnessSpec, LoginState, ResolvedPosture,
};
use fleetor_shell::placement::Host;

/// The two lists the seeder writes containment out of, in the order it reads them
/// (C36) — checkpoint 3's fence and checkpoint 8's socket lever.
///
/// Read off the spec rather than respelled, which is the point: a test that typed
/// `sandbox_mode` here would pass forever after somebody renamed it.
fn containment_keys(spec: &'static HarnessSpec) -> Vec<(&'static str, &'static str)> {
    spec.posture.sandbox_keys.iter().chain(spec.outbound.reachability_keys).copied().collect()
}

/// **Every key FLEETOR writes is read back, and every read-back names a key it
/// writes** (#37, acceptance criterion 1).
///
/// **Both directions, because each one rots differently.** A containment key with no
/// expectation is a setting FLEETOR writes and never checks — the exact state this
/// ticket exists to end, and the state #28 left the trio in. An expectation naming a
/// key nothing writes is worse: it reads as coverage while comparing the vendor
/// against a value that reaches no pane, and it would go on passing after the key it
/// was written for was deleted.
///
/// Run over every registered harness rather than over codex, so a third harness
/// arrives with this rule already applied to it instead of joining the registry with
/// a fence nobody reads back.
#[test]
fn every_containment_key_is_read_back_and_every_read_back_is_a_key() {
    for entry in harness::registered() {
        let spec = entry.spec();
        let written = containment_keys(spec);
        let verified = spec.posture.verified_as;

        // A harness with no diagnostic that reports a resolved posture verifies
        // nothing, and that is an answer rather than a gap: Claude Code has no
        // sandbox of its own and publishes nothing to read back. What it may not do
        // is write containment keys and verify none of them.
        if verified.is_empty() {
            assert!(
                written.is_empty(),
                "`{}` writes containment keys {:?} and reads none of them back — a fence \
                 nobody verifies is the failure #37 exists to end",
                spec.name,
                written.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            );
            continue;
        }

        for (key, value) in &written {
            let rows: Vec<_> =
                verified.iter().filter(|row| row.written_key == *key).collect();
            assert_eq!(
                rows.len(),
                1,
                "`{}` writes `{key} = {value}` and {} rows read it back; it must be \
                 exactly one, or the gate's refusal is ambiguous about which reading \
                 disagreed",
                spec.name,
                rows.len(),
            );
        }

        for row in verified {
            assert!(
                written.iter().any(|(key, _)| *key == row.written_key),
                "`{}` reads back `{}`, which nothing writes into a pane — an expectation \
                 with no key behind it compares the vendor against a value no pane ever \
                 sees",
                spec.name,
                row.written_key,
            );
            // The row name has to be one `ResolvedPosture` actually carries, or the
            // reading is `None` on every machine and the gate refuses every fleet
            // for a typo. `ResolvedPosture::row` is the one spelling of the names.
            let named = ResolvedPosture {
                filesystem: Some("f".into()),
                network: Some("n".into()),
                approval: Some("a".into()),
                schema: None,
            };
            assert!(
                named.row(row.row).is_some(),
                "`{}` reads back a `{}` row, which `ResolvedPosture` does not have",
                spec.name,
                row.row,
            );
        }

        // Two expectations on one row would mean one vendor reading satisfying two
        // different written keys, which is a comparison that cannot be wrong and
        // therefore cannot be useful.
        let mut rows: Vec<&str> = verified.iter().map(|row| row.row).collect();
        rows.sort_unstable();
        let unique = rows.len();
        rows.dedup();
        assert_eq!(unique, rows.len(), "`{}` reads one posture row twice", spec.name);
    }
}

/// **A harness that reads a posture back pins the report shape it read it from**
/// (#37, acceptance criterion 3).
///
/// Every word in `verified_as` is a reading off one document. A vendor that bumps its
/// schema has changed the document, and three readings compared across that change are
/// a comparison of two things that merely share field names. The pin is what turns
/// that into a refusal the operator can act on instead of a green fence nobody can
/// justify — so an expectation list without one is a tripwire with a blind spot
/// exactly where the vendor is most likely to move.
#[test]
fn a_harness_that_reads_a_posture_back_pins_the_schema_it_read_it_from() {
    for entry in harness::registered() {
        let spec = entry.spec();
        assert_eq!(
            spec.posture.verified_as.is_empty(),
            spec.posture.verified_against_schema.is_none(),
            "`{}`: a posture read back without a schema pinned is a reading this build \
             cannot justify, and a schema pinned with nothing read back is a refusal \
             with no subject",
            spec.name,
        );
    }
}

/// **The comparison fires on the readiness itself, and names the key** (#37,
/// acceptance criteria 2 and 4), asserted one level below the gate.
///
/// `fleet.rs`'s own test proves the *gate* refuses; this proves the thing the gate
/// asks. The two are worth having separately because they fail for different reasons:
/// a comparison that stopped firing, and a gate that stopped asking. #35's defect was
/// the second kind and no test of the first kind would have seen it.
#[test]
fn a_resolved_posture_that_disagrees_names_the_key_that_disagreed() {
    for entry in harness::registered() {
        let spec = entry.spec();
        if spec.posture.verified_as.is_empty() {
            continue;
        }

        let agreeing = agreeing_reading(spec);
        assert!(
            agreeing.posture_disagreements().is_empty(),
            "`{}`: the vendor resolving exactly what FLEETOR writes is not a \
             disagreement — a tripwire that fires on agreement is worse than none",
            spec.name,
        );

        for expectation in spec.posture.verified_as {
            // Two ways one key goes wrong, and the vendor does both: a value that
            // moved, and a row that is gone because the key was retired.
            for found in [Some(format!("not-{}", expectation.resolved)), None] {
                let mut broken = agreeing_reading(spec);
                set_row(&mut broken.posture, expectation.row, found.clone());

                let disagreements = broken.posture_disagreements();
                assert_eq!(
                    disagreements.len(),
                    1,
                    "`{}`: one key moved and {} disagreements were reported",
                    spec.name,
                    disagreements.len(),
                );
                let only = &disagreements[0];
                assert_eq!(only.key, expectation.written_key, "the wrong key was named");
                assert_eq!(only.found, found, "the vendor's own reading is carried verbatim");
                assert!(
                    only.sentence().contains(expectation.written_key),
                    "the operator's sentence has to name the key: {}",
                    only.sentence(),
                );
            }
        }

        // The schema, and the fact that it short-circuits: three readings out of a
        // document this build cannot read are not three separate problems.
        let mut moved = agreeing_reading(spec);
        moved.posture.schema = Some("a shape this arc has never seen".to_string());
        let disagreements = moved.posture_disagreements();
        assert_eq!(disagreements.len(), 1, "a schema change is one fact, not one per row");
        assert_eq!(disagreements[0].key, "schemaVersion");
        assert!(
            disagreements[0]
                .sentence()
                .contains(spec.posture.verified_against_schema.expect("pinned above")),
            "the refusal says which schema this build does understand: {}",
            disagreements[0].sentence(),
        );
    }
}

/// **Nothing on the spawn path has a posture to disagree about** (C8, tier 1.4).
///
/// The comparison rides the diagnostic the gate already makes and adds no vendor call
/// of its own — which is only true so long as the reading it consumes stays on
/// [`Host::discover_for_the_gate`]. `Host::discover` is what every spawn runs, and it
/// leaves `harnesses` empty; asserted here as *behaviour* rather than trusted, because
/// a posture tripwire that quietly acquired a subprocess between `fleet send` and a
/// pty would be this arc's own signature failure committed by the check written to
/// prevent it.
///
/// `tests/placement_reads_nothing.rs` holds the other end of the same rule. This is
/// the end that runs.
#[test]
fn the_spawn_paths_own_reading_has_no_posture_to_disagree_about() {
    let machine = Host::discover();
    assert!(
        machine.harnesses.is_empty(),
        "the spawn path's own discovery must carry no harness reading at all, or the \
         posture tripwire has put a subprocess in front of a pty",
    );
}

/// A reading whose vendor resolved exactly what this spec writes.
///
/// Built out of the spec, so it cannot drift from the thing it describes.
fn agreeing_reading(spec: &'static HarnessSpec) -> HarnessReadiness {
    let mut posture = ResolvedPosture {
        schema: spec.posture.verified_against_schema.map(str::to_string),
        ..Default::default()
    };
    for expectation in spec.posture.verified_as {
        set_row(&mut posture, expectation.row, Some(expectation.resolved.to_string()));
    }
    HarnessReadiness {
        login: LoginState::LoggedIn(AccountShape::ApiKey),
        posture,
        ..HarnessReadiness::not_installed(spec)
    }
}

/// Set one row by the name [`ResolvedPosture::rows`] gives it.
///
/// Panics on a name it does not know, which is the point: a fourth row added to
/// `ResolvedPosture` must break this file rather than let it go on asserting about
/// three rows out of four.
fn set_row(posture: &mut ResolvedPosture, row: &str, value: Option<String>) {
    match row {
        "filesystem" => posture.filesystem = value,
        "network" => posture.network = value,
        "approval" => posture.approval = value,
        other => panic!("`ResolvedPosture` grew a `{other}` row this file cannot set"),
    }
}
