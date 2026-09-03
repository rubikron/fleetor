//! The two boundaries dev mode is not allowed to cross (WP-16).
//!
//! Both are properties of *which files mention it*, so both are checked by
//! reading the source rather than by running it — the same move
//! `crates/fleetor-core/src/task.rs` makes with its tripwire list, and for the
//! same reason: the thing worth pinning is that a future session cannot add the
//! branch without the test noticing.
//!
//! 1. **Tier 1.4** — nothing between `fleet send` and a pty may delay, refuse,
//!    reorder, drop or alter a message. A mode readable in there is a mode
//!    something can eventually branch on, so the delivery path does not get to
//!    read one at all.
//! 2. **The veil** (WP-12, open question 4) — dev mode is the *operator's*
//!    posture. `orch` does not learn about it from its brief or from `fleet
//!    roster`, so a pane that infers the evaluator from a banner it was told
//!    about is a failure mode that cannot happen.

use std::path::{Path, PathBuf};

/// Every spelling of the mode a search would plausibly find, lowercased. The
/// underscore form is the config key and the module path; the others are how
/// prose and TypeScript would say it.
const SPELLINGS: [&str; 4] = ["dev_mode", "devmode", "dev mode", "dev-mode"];

/// The repo root — this crate's manifest dir is `src-tauri/`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// Every mention of the mode in `file`, as `line number: line`.
fn mentions(file: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let lowered = line.to_lowercase();
            SPELLINGS.iter().any(|needle| lowered.contains(needle))
        })
        .map(|(i, line)| format!("  {}:{} {}", file.display(), i + 1, line.trim()))
        .collect()
}

fn assert_never_mentions_dev_mode(files: &[&str], why: &str) {
    let root = repo_root();
    let hits: Vec<String> = files.iter().flat_map(|rel| mentions(&root.join(rel))).collect();
    assert!(hits.is_empty(), "{why}\n{}", hits.join("\n"));
}

/// Tier 1.4, as a property of the source rather than a promise about it.
///
/// The list is every file a message actually passes through: the hub that
/// routes it, the bus and store it is recorded in, the wire and message
/// contracts it is spelled in, the delivery loop that types it, the registry
/// that owns the pty, and the CLI that sends it.
#[test]
fn the_delivery_path_cannot_read_dev_mode() {
    assert_never_mentions_dev_mode(
        &[
            "src-tauri/src/deliver.rs",
            "src-tauri/src/pty.rs",
            "crates/fleetor-server/src/hub.rs",
            "crates/fleetor-server/src/bus.rs",
            "crates/fleetor-server/src/lib.rs",
            "crates/fleetor-core/src/message.rs",
            "crates/fleetor-core/src/wire.rs",
            "crates/fleetor-core/src/command.rs",
            "crates/fleetor-cli/src/main.rs",
        ],
        "Tier 1.4: nothing between `fleet send` and a pty may know what mode the app is in. \
         A path that can read a mode is a path that can branch on one, and a delivery that \
         can be branched on is the gate D-034 refused twice. Move the branch to the caller.",
    );
}

/// Every file that becomes words a pane is told. `orch`'s whole world is its
/// brief and what `fleet roster` answers.
///
/// `prompts/scaffolding.md` joined this list in WP-15: it is composed into
/// *both* briefs (D-043) and was the one fragment nobody had checked.
///
/// `prompts/critic.md` joined it in WP-20 (D-076). The Critic is not in the
/// fleet, so the inference this test guards against — a pane reading a banner
/// and reasoning about what sits behind it — is not the reason it is here; it
/// has no route to tell anyone what it inferred. It is here because it is prose
/// a terminal is handed, it has no business naming the mode, and a list of
/// "every file that becomes words a pane is told" that quietly excluded one is a
/// list the next reader has to be told about. Adding it cost nothing: it names
/// no spelling of the mode.
const WHAT_A_PANE_IS_TOLD: [&str; 9] = [
    "prompts/orch.md",
    "prompts/worker.md",
    "prompts/delivery-contract.md",
    "prompts/broadcast-rule.md",
    "prompts/vision-tenets.md",
    "prompts/scaffolding.md",
    "prompts/critic.md",
    "crates/fleetor-core/src/brief.rs",
    "crates/fleetor-core/src/pane.rs",
];

/// The list above minus `crates/fleetor-core/src/pane.rs` and
/// `prompts/critic.md`, for the evaluator's half of the veil only.
///
/// **Read the two tests below before concluding either omission is a hole.**
///
/// `pane.rs` stays on the list above — it must never mention the *mode* — and
/// comes off this one because WP-15 put the name of a non-member identity in it,
/// which is the thing the file is for. What replaced the grep is
/// [`nothing_a_pane_can_read_ever_names_the_evaluator`], which checks the
/// rendered artifacts rather than the source that produces them.
///
/// **`prompts/critic.md` is off this list on purpose, and the reason is one
/// word** (WP-20, D-076). Six of the seven needles below never appear in it, and
/// [`the_critics_brief_is_openly_named_and_names_no_evaluator_vocabulary`]
/// asserts every one of those six on that file. The seventh is `answer key`, and
/// the Critic's brief contains it in the sentence that states the Critic's own
/// limit — *"You have no answer key"* — which is D10's own phrase for why this
/// pane judges the run and never the work. That is not the evaluator leaking
/// into the fleet's prose; it is the opposite claim, made by a pane that is not
/// in the fleet and cannot tell it anything. Putting the file on this list would
/// leave exactly two moves: edit the prompt the spike converged on, or widen the
/// needle so it stops matching what it was written to catch. **Neither is worth
/// a grep on a file no fleet pane reads.**
const WHAT_A_PANE_IS_TOLD_IN_PROSE: [&str; 7] = [
    "prompts/orch.md",
    "prompts/worker.md",
    "prompts/delivery-contract.md",
    "prompts/broadcast-rule.md",
    "prompts/vision-tenets.md",
    "prompts/scaffolding.md",
    "crates/fleetor-core/src/brief.rs",
];

/// The veil, half one: the mode itself.
#[test]
fn no_pane_is_briefed_about_dev_mode() {
    assert_never_mentions_dev_mode(
        &WHAT_A_PANE_IS_TOLD,
        "WP-12 open question 4: dev mode is visible in the operator's UI and absent from \
         every word a pane is told. Teaching `orch` the mode exists invites it to infer the \
         evaluator behind it — and the veil is far cheaper to keep than to re-establish.",
    );
}

/// **The veil, half two: the evaluator itself** (WP-13).
///
/// The mode was the indirect leak; this is the direct one. WP-13's `fleet
/// handoff` is the first thing a pane is taught that something *else* will
/// eventually react to, and the whole design of that reaction depends on `orch`
/// not knowing it is coming: an orchestrator told its handoff will be read by an
/// evaluator writes the handoff for the evaluator, and the run stops being
/// evidence of how the fleet actually works.
///
/// The verb is taught as what it honestly is from the fleet's side — a report to
/// the operator that the goal is met — so this checks the vocabulary of the
/// thing on the other side of it never appears in any of those files. Note that
/// `review` is deliberately **not** on this list: peer review is a real, taught,
/// worker-facing thing (WP-06), and conflating the two words is how this test
/// would start failing for the wrong reason.
/// **WP-15 took `pane.rs` off this test's file list and put something stricter
/// in its place. Read this before restoring it.**
///
/// The evaluator has to be addressable in both directions — first contact is a
/// message to `orch`, and the retro is a conversation `orch` answers — and every
/// name in this system's record is a `PaneId`: the CLI parses one, `Hello`
/// declares one, `FleetEvent::Message` carries one from and to, and `fleet
/// reply` resolves through a `HashMap<PaneId, PaneId>`. There is no way to give
/// the evaluator a name without `pane.rs` containing it, and the alternatives
/// are worse in kind rather than in degree: a spelling chosen to slip past this
/// grep is the workaround `building.md` §6 bans by name, and reusing `operator`
/// would put a lie in the log about who said what.
///
/// So the grep on that one file is replaced by
/// [`nothing_a_pane_can_read_ever_names_the_evaluator`], which asserts the
/// property this grep was a proxy for: not "the source does not contain the
/// word" but "no pane is ever shown it". That is strictly more coverage —
/// `pane.rs` could always have rendered a name it never spelled literally, and
/// the grep would have passed.
#[test]
fn no_pane_is_briefed_about_the_evaluator() {
    const EVALUATOR_WORDS: [&str; 6] =
        ["evaluat", "the retro", "answer key", "rubric", "grader", "proposal ledger"];
    let root = repo_root();
    let hits: Vec<String> = WHAT_A_PANE_IS_TOLD_IN_PROSE
        .iter()
        .flat_map(|rel| {
            let file = root.join(rel);
            let text = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
            text.lines()
                .enumerate()
                .filter(|(_, line)| {
                    let lowered = line.to_lowercase();
                    EVALUATOR_WORDS.iter().any(|needle| lowered.contains(needle))
                })
                .map(|(i, line)| format!("  {}:{} {}", file.display(), i + 1, line.trim()))
                .collect::<Vec<String>>()
        })
        .collect();
    assert!(
        hits.is_empty(),
        "WP-12's veil: `orch` does not learn that an evaluator exists until the retro starts, \
         and `fleet handoff` is taught as a report to the operator and nothing more. A pane \
         that knows its handoff will be assessed optimizes for the assessment instead of \
         doing the work.\n{}",
        hits.join("\n"),
    );
}

/// **The veil, checked on what a pane actually sees rather than on what the
/// source says** (WP-15). This is what replaced `pane.rs`'s row on
/// [`WHAT_A_PANE_IS_TOLD_IN_PROSE`] — see the test above for why, and note that
/// each of these four assertions could have failed while that grep passed.
///
/// A pane's whole world is four things, and none of them may contain the name:
///
///  1. **The rendered orchestrator brief**, `{workers}` included.
///  2. **A rendered worker brief**, `{peers}` included.
///  3. **The pane roster** — what spawns, what a broadcast fans out over, and
///     what a peer list is built from.
///  4. **The error a mistyped pane name produces.** The one place a name nobody
///     was briefed on leaks by accident, because a model reads its own stderr
///     and acts on what it finds there.
#[test]
fn nothing_a_pane_can_read_ever_names_the_evaluator() {
    use fleetor_core::brief::{orch_brief, worker_brief};
    use fleetor_core::pane::{PaneId, WORKER_SLOTS};

    const NAME: &str = "evaluator";
    let roster = PaneId::roster(&WORKER_SLOTS);

    // 3. The roster first — everything below is rendered from it.
    assert!(
        !roster.iter().any(|p| p.is_evaluator()),
        "the pane roster is what spawns and what a broadcast reaches: {roster:?}",
    );
    assert!(roster.iter().all(|p| p.is_fleet_member()), "the roster is the fleet, exactly");

    // 1 and 2. The briefs, rendered the way a real spawn renders them.
    let cwd = "/tmp/fleetor-veil-test";
    for (who, brief) in [
        ("orch", orch_brief(&roster, cwd)),
        ("worker-1", worker_brief(PaneId::Worker(1), &roster, cwd)),
    ] {
        assert!(
            !brief.to_lowercase().contains(NAME),
            "{who}'s rendered brief names the evaluator — the veil is what makes first \
             contact evidence rather than performance",
        );
    }

    // 4. The parse error. `pane.rs` documents that list as complete; this is
    // what makes the documentation load-bearing.
    let err = "sidebar".parse::<PaneId>().unwrap_err().to_string();
    assert!(
        !err.to_lowercase().contains(NAME),
        "a mistyped pane name must not teach the sender a name it was never briefed on: {err}",
    );

    // And the name still resolves — not-enumerated is not the same as
    // not-addressable. The retro is a conversation `orch` has to be able to
    // answer once it has been spoken to.
    assert_eq!(NAME.parse::<PaneId>().unwrap(), PaneId::Evaluator);
    assert!(PaneId::Evaluator.has_pty(), "a real terminal, so `accepted` is the honest word");
    assert!(!PaneId::Evaluator.is_fleet_member());
}

// --- the Critic, which is openly named (WP-20, D-076) ---------------------------

/// **The Critic is the opposite of a veiled pane, and this test says so from both
/// sides.**
///
/// The evaluator is hidden because the veil is what makes a handoff evidence
/// rather than performance. Nothing about the Critic depends on the fleet not
/// knowing it exists — it is a product feature, in the rail, with its brief in
/// `prompts/` for the operator to rewrite. So there is no grep here saying "the
/// word `critic` must not appear"; there is a grep saying the *evaluator's*
/// vocabulary must not appear in the Critic's brief, which is a different claim
/// and the one that actually matters.
///
/// Six of the seven needles from [`no_pane_is_briefed_about_the_evaluator`] are
/// checked here on the Critic's brief. The seventh, `answer key`, is left out
/// with its reason on [`WHAT_A_PANE_IS_TOLD_IN_PROSE`] — it appears in that brief
/// as the Critic's statement of its own limit, and it is the last phrase that
/// should ever be edited out of it.
#[test]
fn the_critics_brief_is_openly_named_and_names_no_evaluator_vocabulary() {
    const NOT_THE_CRITICS_WORDS: [&str; 5] =
        ["evaluat", "the retro", "rubric", "grader", "proposal ledger"];
    let file = repo_root().join("prompts/critic.md");
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));

    let hits: Vec<String> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let lowered = line.to_lowercase();
            NOT_THE_CRITICS_WORDS.iter().any(|needle| lowered.contains(needle))
        })
        .map(|(i, line)| format!("  prompts/critic.md:{} {}", i + 1, line.trim()))
        .collect();
    assert!(
        hits.is_empty(),
        "the Critic reports what the fleet did and has nothing to say about the pane that \
         grades it. This brief is a file on disk in a directory every pane can read by \
         absolute path, and it is handed to a `claude` the operator can talk to — it is the \
         one new place WP-15's vocabulary could reach a reader who was never meant to have \
         it.\n{}",
        hits.join("\n"),
    );

    // And the one word that *is* in there is in there for the right reason: it is
    // the Critic's own limit, not a description of something else in the app.
    assert!(
        text.to_lowercase().contains("you have no answer key"),
        "the sentence D10 is built on — the Critic judges the run because it cannot judge \
         the work — must survive any trim of this brief",
    );
}

/// **The tripwires that hide the evaluator did not start matching the Critic.**
///
/// Two claims, and only the second is obvious. The first is that the evaluator's
/// needles are still the evaluator's: `no_pane_is_briefed_about_the_evaluator`
/// would pass just as happily against a needle list that had been widened,
/// narrowed or emptied to make new code compile, so the words are pinned here as
/// literals. The second is that none of them is a word the Critic answers to — a
/// needle matching both names would make every one of those greps ambiguous,
/// which is the collision D7 renamed this pane to avoid.
#[test]
fn the_evaluators_needles_still_mean_the_evaluator_and_never_the_critic() {
    use fleetor_core::pane::PaneId;
    const NEEDLES: [&str; 6] =
        ["evaluat", "the retro", "answer key", "rubric", "grader", "proposal ledger"];
    let critic = PaneId::Critic.to_string();
    for needle in NEEDLES {
        assert!(
            !critic.contains(needle) && !needle.contains(&critic),
            "`{needle}` overlaps the Critic's own name: every veil grep that uses it would \
             start firing on a pane that is deliberately named in the open (D7)",
        );
    }
    assert!(PaneId::Evaluator.to_string().contains(NEEDLES[0]), "still the name it was written for");
}

/// **The Critic is in no enumeration of the fleet, and it is not hidden — those
/// are different facts and both are true.**
///
/// The sibling of [`nothing_a_pane_can_read_ever_names_the_evaluator`], asserting
/// the half that is shared (nothing fans out to it, nothing lists it, no brief
/// names it) without the half that is not: the fleet may see the word `critic`
/// anywhere it likes, because there is nothing to leak.
#[test]
fn no_brief_and_no_roster_ever_reaches_the_critic() {
    use fleetor_core::brief::{orch_brief, worker_brief};
    use fleetor_core::pane::{PaneId, WORKER_SLOTS};

    const NAME: &str = "critic";
    let roster = PaneId::roster(&WORKER_SLOTS);

    assert!(
        !roster.iter().any(|p| p.is_critic()),
        "the pane roster is what spawns and what a broadcast reaches: {roster:?}",
    );
    // Deliberately a path with none of this pane's letters in it: a fixture whose
    // own name contained the needle would fail this test for a reason that has
    // nothing to do with the briefs.
    let cwd = "/tmp/fleetor-veil-test";
    for (who, brief) in [
        ("orch", orch_brief(&roster, cwd)),
        ("worker-1", worker_brief(PaneId::Worker(1), &roster, cwd)),
    ] {
        assert!(
            !brief.to_lowercase().contains(NAME),
            "{who}'s rendered brief names the Critic. It is not a secret, but it is not a \
             peer either — a pane told about it would try to send to it, and it has no \
             socket to be reached on",
        );
    }
    let err = "sidebar".parse::<PaneId>().unwrap_err().to_string();
    assert!(
        !err.to_lowercase().contains(NAME),
        "the parse error offers the fleet's own names and only those: {err}",
    );

    // Addressable by the app that spawns it, and outside the fleet's roster.
    assert_eq!(NAME.parse::<PaneId>().unwrap(), PaneId::Critic);
    assert!(PaneId::Critic.has_pty(), "a real, typeable terminal");
    assert!(!PaneId::Critic.is_fleet_member());
}
