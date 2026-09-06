//! **Harness conformance, checkpoints 1–7** (WP-25, issue #15; M23, C13, C17,
//! C19, C20, C25).
//!
//! One pass per registered harness over the same seven assertions, driven through
//! [`placement::place`] against a scratch layout. Checkpoints 8–14 are the file
//! next door; the driver both share is `tests/conformance/mod.rs`, and its header
//! is where the shape, the rule and the extension point are written down.
//!
//! **A suite with one registered harness is the point, not a limitation.** It is
//! green with Claude Code as the sole registered harness, before any second
//! harness exists — otherwise it would be a description of the second harness
//! wearing an abstraction's clothes, which is the failure this arc was reordered
//! to avoid (C18, C20).
//!
//! **This is also the safety net for the migrate batches.** Every assertion here
//! is caller-observable — the command that will be run, the files written into a
//! scratch layout, the notices returned — and none of them reads
//! `CLAUDE_CODE_SPEC` and agrees with it. The migrations move the literals in
//! `spawn.rs` and its siblings onto the spec underneath a `place` whose observable
//! behaviour does not change, so this file stays green batch to batch, and a batch
//! that breaks behaviour fails here loudly.

mod conformance;

use std::path::Path;

use conformance::{
    arg_after, argv_of, env_on, files_containing, for_each_registered, is_non_empty_dir, roots_in,
    ORCH_BRIEF_MARK, WORKER_BASE_URL, WORKER_BRIEF_MARK, WORKER_KEY, WORKER_MODEL, WORKER_POSTURE,
};
use fleetor_shell::placement::harness::registered;

// --- the suite's own shape ------------------------------------------------------

/// **Every registered harness gets a pass, and there is at least one.**
///
/// The meta-assertion the other seven rest on: a registry that lost its last entry
/// would make every checkpoint below vacuously green, and a registry that gained
/// one without `place` learning to select it would make them green about the wrong
/// harness. The first is caught here; the second is caught inside the driver,
/// which asserts that each seat really was placed as the harness its pass is for.
#[test]
fn the_suite_runs_one_pass_per_registered_harness() {
    let passes = for_each_registered("shape", |pass| {
        assert!(!pass.spec.name.is_empty(), "a harness with no name cannot be written down");
    });
    assert_eq!(
        passes,
        registered().len(),
        "one pass per registered harness, no more and no fewer",
    );
    assert!(passes >= 1, "the suite must have something to say");
}

// --- checkpoint 1 ---------------------------------------------------------------

/// **Checkpoint 1 — program and base arguments.**
///
/// The binary every pane of this harness runs is the one the spec names, and
/// whatever it says every pane gets comes first, in order, before any per-seat
/// argument. Asserted on the command `place` returns, so a harness that names one
/// program and launches another fails here rather than at the first spawn.
///
/// The stand-in override is deliberately out of the way: this pass's host has no
/// `pane_program`, so the program on the command really is the harness's own.
#[test]
fn checkpoint_1_the_program_and_base_arguments_are_what_the_spec_names() {
    for_each_registered("cp1", |pass| {
        let program = pass.spec.program.bin;
        assert!(
            !program.trim().is_empty(),
            "{}: a harness with no program is a pane that cannot start",
            pass.spec.name,
        );

        for (seat, placed) in pass.seats() {
            let argv = argv_of(placed);
            assert_eq!(
                argv.first().map(String::as_str),
                Some(program),
                "{}/{seat}: the command must run the program the spec names",
                pass.spec.name,
            );

            let base = pass.spec.program.base_args;
            let carried: Vec<&str> =
                argv.iter().skip(1).take(base.len()).map(String::as_str).collect();
            assert_eq!(
                carried, base,
                "{}/{seat}: every pane of this harness gets its base arguments first, in order",
                pass.spec.name,
            );
        }
    });
}

// --- checkpoint 2 ---------------------------------------------------------------

/// **Checkpoint 2 — brief carrier.**
///
/// D-042 holds across every harness — one orchestrator brief, one worker brief,
/// and only the carrier varies — so what is asserted is transport: the brief the
/// caller handed in reaches the pane, behind whichever carrier the spec names, and
/// each seat gets *its own* brief rather than the same one twice.
///
/// The second half is the one that shows up in an operator's `git status`:
/// `writes_into_worktree` is a claim about whether a brief file lands inside the
/// pane's checkout, and it is checked by looking for the brief's own mark in the
/// files under that checkout.
///
/// **`replaces_system_prompt` is not asserted here, and cannot be.** It is a claim
/// about what the *vendor* does with the brief once it has it — whether the fleet's
/// instructions replace the built-in prompt or sit underneath it (D-043) — and
/// nothing `place` returns can show that. It is C13's vendor tier's to measure,
/// off the wire, against the vendor's own binary.
#[test]
fn checkpoint_2_the_brief_reaches_the_pane_by_the_carrier_the_spec_names() {
    for_each_registered("cp2", |pass| {
        let brief = &pass.spec.brief;
        assert!(
            brief.argv_flag.is_some() ^ brief.config_key.is_some(),
            "{}: exactly one of argv_flag and config_key carries the brief",
            pass.spec.name,
        );

        for (seat, placed) in pass.seats() {
            let mark = if seat == "orch" { ORCH_BRIEF_MARK } else { WORKER_BRIEF_MARK };

            match (brief.argv_flag, brief.config_key) {
                (Some(flag), _) => {
                    let carried = arg_after(placed, flag).unwrap_or_else(|| {
                        panic!("{}/{seat}: nothing follows {flag} on the command", pass.spec.name)
                    });
                    assert!(
                        carried.contains(mark),
                        "{}/{seat}: the brief behind {flag} is not the one the caller handed in",
                        pass.spec.name,
                    );
                }
                (_, Some(key)) => {
                    // A harness that carries the brief through its config dir has
                    // to have named the file, and the file has to hold the brief.
                    let dir = pass.config_dir(placed);
                    assert!(
                        !files_containing(&dir, key).is_empty(),
                        "{}/{seat}: nothing in {} names {key}",
                        pass.spec.name,
                        dir.display(),
                    );
                    assert!(
                        !files_containing(&dir, mark).is_empty(),
                        "{}/{seat}: the brief was never written into the config dir",
                        pass.spec.name,
                    );
                }
                (None, None) => unreachable!("the exactly-one assertion above"),
            }

            // The other seat's brief did not go to this one.
            let other = if seat == "orch" { WORKER_BRIEF_MARK } else { ORCH_BRIEF_MARK };
            assert!(
                !argv_of(placed).iter().any(|a| a.contains(other)),
                "{}/{seat}: this seat was handed the other seat's brief",
                pass.spec.name,
            );

            // And whether it landed in the pane's own checkout is the spec's claim.
            let cwd = pass.cwd_of(seat);
            let inside = files_containing(&cwd, mark);
            if brief.writes_into_worktree {
                assert!(
                    !inside.is_empty(),
                    "{}/{seat}: the spec says the carrier writes into the checkout, and nothing did",
                    pass.spec.name,
                );
            } else {
                assert!(
                    inside.is_empty(),
                    "{}/{seat}: a brief file in the checkout shows up in the operator's \
                     git status and has to be excluded — found {inside:?} in {}",
                    pass.spec.name,
                    cwd.display(),
                );
            }
        }
    });
}

// --- checkpoint 3 ---------------------------------------------------------------

/// **Checkpoint 3 — model flag and permission or sandbox posture.**
///
/// Two things set on the same command: which model an unattended pane runs, and
/// how far it may act without being asked. Both are asserted as *values that
/// travelled* — the pass hands in a model, an endpoint and a posture nothing would
/// produce by accident — and both are asserted on the asymmetry that is the
/// product: the operator's own seat runs their account and their model and is
/// watched by a human, so it gets neither (D-030, D-052).
///
/// **The unattended seat must be bounded by something the spec names.** A harness
/// with no permission flag and no sandbox keys has answered this checkpoint with a
/// stub, and a stub here is a pane that acts unattended with nothing said about how
/// far it may go.
#[test]
fn checkpoint_3_the_model_and_the_posture_reach_the_unattended_seat_alone() {
    for_each_registered("cp3", |pass| {
        let posture = &pass.spec.posture;
        assert!(
            !(posture.model_env.is_some() && posture.model_flag.is_some()),
            "{}: one model channel, not two — a harness that sets both can disagree with itself",
            pass.spec.name,
        );
        assert!(
            posture.permission_flag.is_some() || !posture.sandbox_keys.is_empty(),
            "{}: an unattended pane bounded by nothing the spec names is checkpoint 3 stubbed",
            pass.spec.name,
        );

        if let Some(var) = posture.model_env {
            assert_eq!(
                env_on(&pass.worker, var).as_deref(),
                Some(WORKER_MODEL),
                "{}: the worker runs the model the caller named",
                pass.spec.name,
            );
            assert_ne!(
                env_on(&pass.orch, var).as_deref(),
                Some(WORKER_MODEL),
                "{}: the operator's own seat runs their model, never the fleet's worker model",
                pass.spec.name,
            );
        }
        if let Some(flag) = posture.model_flag {
            assert_eq!(
                arg_after(&pass.worker, flag).as_deref(),
                Some(WORKER_MODEL),
                "{}: the worker runs the model the caller named",
                pass.spec.name,
            );
            assert!(
                !argv_of(&pass.orch).iter().any(|a| a == flag),
                "{}: the operator's own seat is not given a model flag",
                pass.spec.name,
            );
        }

        if let Some(flag) = posture.permission_flag {
            assert_eq!(
                arg_after(&pass.worker, flag).as_deref(),
                Some(WORKER_POSTURE),
                "{}: the worker's posture is the one the caller named, not a default",
                pass.spec.name,
            );
            assert!(
                !argv_of(&pass.orch).iter().any(|a| a == flag),
                "{}: the operator's own seat is watched by a human and gets no posture flag",
                pass.spec.name,
            );
        }

        // Tier 1.7 reads these: whatever a harness puts here narrows what a pane
        // may do, so they have to have actually been written where the pane will
        // read them.
        for (key, value) in pass.spec.posture.sandbox_keys {
            let dir = pass.config_dir(&pass.worker);
            assert!(
                !files_containing(&dir, key).is_empty()
                    && !files_containing(&dir, value).is_empty(),
                "{}: sandbox key {key} = {value} is in the spec and not in {}",
                pass.spec.name,
                dir.display(),
            );
        }
    });
}

// --- checkpoint 4 ---------------------------------------------------------------

/// **Checkpoint 4 — configuration directory and its seeding.**
///
/// The L1 requirement is this checkpoint's whole reason for existing: an unseeded
/// config dir is a pane that boots into onboarding while every `fleet send`
/// reports `accepted`. So the assertions are about the directory the pane was
/// actually pointed at — read off the command, never re-derived from the layout —
/// and about the file inside it having the keys the spec says get a pane to its
/// prompt.
///
/// **The merge half is asserted across a target switch**, because that is the one
/// way it is ever observed to fail: the trust record is keyed by project path, a
/// fleet pointed at a new target re-seeds every pane's config dir, and a seed that
/// clobbers instead of merging works exactly once.
#[test]
fn checkpoint_4_the_config_dir_is_pointed_at_seeded_and_merged_into() {
    for_each_registered("cp4", |pass| {
        let config = &pass.spec.config_dir;
        assert!(!config.env_var.trim().is_empty(), "{}: checkpoint 4 has no name", pass.spec.name);
        assert!(!config.seed_file.trim().is_empty(), "{}: nothing to seed", pass.spec.name);
        assert!(
            !config.seed_keys.is_empty(),
            "{}: a seed that sets no key is a pane that boots into onboarding (L1)",
            pass.spec.name,
        );

        for (seat, placed) in pass.seats() {
            let dir = pass.config_dir(placed);
            assert!(
                pass.is_contained(&dir),
                "{}/{seat}: the pane's config dir is outside the layout it was handed: {}",
                pass.spec.name,
                dir.display(),
            );
            assert!(
                dir.is_dir(),
                "{}/{seat}: {} was named on the command and never made",
                pass.spec.name,
                dir.display(),
            );

            let text = pass.config_text(placed, config.seed_file);
            assert!(
                !text.trim().is_empty(),
                "{}/{seat}: the seed file is empty",
                pass.spec.name,
            );
            for key in config.seed_keys {
                assert!(
                    text.contains(key),
                    "{}/{seat}: the seed does not set {key}",
                    pass.spec.name,
                );
            }
        }

        if config.seed_merges {
            // The key the first placement was seeded under, before the switch. Its
            // *shape* is checkpoint 14's claim; here it is only the handle on what
            // was there before.
            let first = pass.harness.project_key(&pass.worker_cwd);
            let before = pass.config_text(&pass.worker, config.seed_file);
            assert!(before.contains(&first), "{}: the first target was never seeded", pass.spec.name);

            let (_other, placed) = pass.place_worker_against("second-repo");
            let after = pass.config_text(&placed, config.seed_file);
            assert!(
                after.contains(&first),
                "{}: switching targets clobbered the first target's record — a seed that \
                 replaces instead of merging works exactly once",
                pass.spec.name,
            );
        }
    });
}

// --- checkpoint 5 ---------------------------------------------------------------

/// **Checkpoint 5 — credential wiring and environment scrub.**
///
/// D-062 across every harness: a worker holds the fleet's credential, never the
/// operator's. Both halves are asserted, and the second is the one that is easier
/// to get wrong — what has to be *removed* from an inherited environment so the
/// operator's own credential cannot leak into a fenced pane (L2).
///
/// **What the scrub half can and cannot say here, stated plainly.** `place`
/// returns a `CommandBuilder` whose environment is the app's own, snapshotted, plus
/// placement's overrides — and `env_remove` deletes an entry rather than marking
/// it, so a name that was removed and a name the machine never had are the same
/// observation. Distinguishing them needs the name to be in the process
/// environment when the command is built, which means a test that sets one; this
/// suite does not (`tests/placement.rs` makes that exception once, deliberately,
/// for `ANTHROPIC_API_KEY`). So what is asserted is that **no scrubbed name has a
/// value on a worker's command** — which catches a harness that hands a fenced
/// pane the operator's own credential, and does not catch a missing `env_remove`
/// on a machine that never exported it.
#[test]
fn checkpoint_5_the_worker_holds_the_fleets_credential_and_the_operators_is_removed() {
    for_each_registered("cp5", |pass| {
        let creds = &pass.spec.credentials;
        assert!(
            creds.token_env.is_some() || !creds.provider_keys.is_empty(),
            "{}: a worker with no credential channel at all cannot authenticate",
            pass.spec.name,
        );

        if let Some(var) = creds.base_url_env {
            assert_eq!(
                env_on(&pass.worker, var).as_deref(),
                Some(WORKER_BASE_URL),
                "{}: the worker talks to the endpoint the caller named",
                pass.spec.name,
            );
        }
        if let Some(var) = creds.token_env {
            assert_eq!(
                env_on(&pass.worker, var).as_deref(),
                Some(WORKER_KEY),
                "{}: the worker authenticates with the key from the host",
                pass.spec.name,
            );
            assert_ne!(
                env_on(&pass.orch, var).as_deref(),
                Some(WORKER_KEY),
                "{}: the operator's own seat runs their login, not the fleet's credential",
                pass.spec.name,
            );
        }
        for (key, value) in creds.provider_keys {
            let dir = pass.config_dir(&pass.worker);
            assert!(
                !files_containing(&dir, key).is_empty()
                    && !files_containing(&dir, value).is_empty(),
                "{}: provider key {key} is in the spec and not in {}",
                pass.spec.name,
                dir.display(),
            );
        }

        assert!(
            !creds.scrubbed_env.is_empty(),
            "{}: a worker inherits the app's environment, so a scrub that removes nothing \
             is checkpoint 5 stubbed",
            pass.spec.name,
        );
        for name in creds.scrubbed_env {
            assert_eq!(
                env_on(&pass.worker, name),
                None,
                "{}: {name} is on a fenced pane's command, and checkpoint 5 says it is scrubbed",
                pass.spec.name,
            );
        }
    });
}

// --- checkpoint 6 ---------------------------------------------------------------

/// **Checkpoint 6 — config and credential isolation mechanism** (C17, generalized
/// from "private HOME seeding").
///
/// The checkpoint C17 renamed, because the old name described one vendor's
/// mechanism as a harness universal it is not. What is asserted is therefore the
/// three mechanisms separately: the variable that relocates *configuration*, the
/// variable that selects the *credential* namespace, and the private `HOME` — with
/// `credentials_follow_home` deciding which of the last two a fenced pane's login
/// actually arrives through.
///
/// **The pass places against a machine with nothing on it**, which is what makes
/// `seeds_from_operator: false` a thing a test can say rather than assume: there
/// is no operator home to snapshot, and the pane reaches its prompt anyway.
#[test]
fn checkpoint_6_configuration_and_credentials_are_isolated_by_the_named_mechanism() {
    for_each_registered("cp6", |pass| {
        let isolation = &pass.spec.isolation;
        assert!(
            !isolation.config_env.trim().is_empty(),
            "{}: a harness that relocates no configuration shares the operator's",
            pass.spec.name,
        );

        for (seat, placed) in pass.seats() {
            let dir = env_on(placed, isolation.config_env).unwrap_or_else(|| {
                panic!("{}/{seat}: {} is not on the command", pass.spec.name, isolation.config_env)
            });
            assert!(
                pass.is_contained(Path::new(&dir)),
                "{}/{seat}: configuration is relocated outside the layout: {dir}",
                pass.spec.name,
            );
        }

        if let Some(cred) = isolation.credential_env {
            assert!(
                env_on(&pass.orch, cred).is_some(),
                "{}: the attended seat must be pointed at the operator's own credential \
                 namespace, or it boots into an empty one looking healthy",
                pass.spec.name,
            );
            assert_eq!(
                env_on(&pass.worker, cred),
                None,
                "{}: a fenced pane is never handed the key to the operator's login",
                pass.spec.name,
            );
            assert!(
                pass.spec.credentials.scrubbed_env.contains(&cred),
                "{}: {cred} is removed from a worker's command, so checkpoint 5 has to say so",
                pass.spec.name,
            );
        }

        if isolation.private_home {
            let home = env_on(&pass.worker, "HOME")
                .unwrap_or_else(|| panic!("{}: a fenced pane is given a HOME", pass.spec.name));
            assert!(
                pass.is_contained(Path::new(&home)),
                "{}: the Fence's private HOME is outside the layout: {home}",
                pass.spec.name,
            );
            assert!(Path::new(&home).is_dir(), "{}: the private HOME was never made", pass.spec.name);
            assert_ne!(
                env_on(&pass.orch, "HOME").as_deref(),
                Some(home.as_str()),
                "{}: the Fence is the worker's, not the operator's seat's",
                pass.spec.name,
            );
        }

        if isolation.credentials_follow_home {
            // Then the private HOME *is* the credential mechanism, and it has to be
            // both given and seeded from somewhere, or the pane has no login at all.
            assert!(
                isolation.private_home && isolation.seeds_from_operator,
                "{}: a harness whose login is reachable only through HOME needs a private \
                 one, seeded",
                pass.spec.name,
            );
        } else {
            // C17's own measurement, asserted rather than restated: a fenced pane of
            // this harness keeps a private HOME and still has a credential, which
            // arrives through checkpoint 5's channel or the namespace variable above.
            let reachable = pass
                .spec
                .credentials
                .token_env
                .is_some_and(|var| env_on(&pass.worker, var).is_some())
                || !pass.spec.credentials.provider_keys.is_empty();
            assert!(
                reachable,
                "{}: this harness's login does not follow HOME, so a fenced pane's \
                 credential has to arrive some other way — and none did",
                pass.spec.name,
            );
        }

        assert!(
            pass.host.operator_home.is_none(),
            "the pass places against a machine with nothing, which is what the next \
             assertion is worth anything for",
        );
        if isolation.seeds_from_operator {
            panic!(
                "{}: this harness seeds its isolated directory from the operator's own, \
                 and the driver places against a machine that has none — give \
                 `conformance::Pass` an operator directory to snapshot before registering it",
                pass.spec.name,
            );
        }
        for (seat, placed) in pass.seats() {
            let dir = pass.config_dir(placed);
            assert!(
                is_non_empty_dir(&dir),
                "{}/{seat}: the isolated directory was created fresh and left empty, so the \
                 pane has nothing to boot on",
                pass.spec.name,
            );
        }
    });
}

// --- checkpoint 7 ---------------------------------------------------------------

/// **Checkpoint 7 — write-guardrail install.**
///
/// Every pane, every harness, gets a pre-edit refusal that no pane can rewrite.
/// What varies is the settings file, the event and the tool matcher; the script,
/// the interpreter and the journal are the fleet's and identical everywhere, which
/// is why the roots are read out of the installed file by the hook's own spelling
/// rather than by any harness's.
///
/// **Tier 1.7 is the second half of this test.** Auto-approve never exceeds the
/// pane's own worktree: every root the installed hook was given is inside the
/// layout or the target `place` was handed, and the pane's own working directory
/// is one of them. A harness that installed the guardrail with wider roots — or
/// installed it on the unattended seat alone — fails here.
#[test]
fn checkpoint_7_every_seat_gets_the_write_guardrail_with_roots_no_wider_than_its_own() {
    for_each_registered("cp7", |pass| {
        let install = &pass.spec.guardrail;
        for (what, value) in [
            ("settings_file", install.settings_file),
            ("hook_event", install.hook_event),
            ("tool_matcher", install.tool_matcher),
            ("hook_file", install.hook_file),
        ] {
            assert!(
                !value.trim().is_empty(),
                "{}: checkpoint 7's {what} is empty, which is a pane with no pre-edit refusal",
                pass.spec.name,
            );
        }

        for (seat, placed) in pass.seats() {
            let dir = pass.config_dir(placed);
            assert!(
                dir.join(install.hook_file).is_file(),
                "{}/{seat}: the hook script itself is not in {}",
                pass.spec.name,
                dir.display(),
            );

            let settings = pass.config_text(placed, install.settings_file);
            for (what, needle) in [
                ("hook event", install.hook_event),
                ("tool matcher", install.tool_matcher),
                ("hook file", install.hook_file),
            ] {
                assert!(
                    settings.contains(needle),
                    "{}/{seat}: {} does not register the {what} the spec names ({needle})",
                    pass.spec.name,
                    install.settings_file,
                );
            }

            let roots = roots_in(&settings);
            assert!(
                !roots.is_empty(),
                "{}/{seat}: the hook was installed with no write roots at all",
                pass.spec.name,
            );
            let cwd = pass.cwd_of(seat);
            let resolved =
                std::fs::canonicalize(&cwd).unwrap_or_else(|_| cwd.clone());
            assert!(
                roots.iter().any(|root| {
                    let root = Path::new(root);
                    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()) == resolved
                }),
                "{}/{seat}: the pane cannot write in its own working directory {} — roots \
                 were {roots:?}",
                pass.spec.name,
                cwd.display(),
            );
            for root in &roots {
                assert!(
                    pass.is_contained(Path::new(root)),
                    "{}/{seat}: Tier 1.7 — the guardrail was given {root}, which is outside \
                     the layout and the target placement was handed",
                    pass.spec.name,
                );
            }
        }
    });
}
