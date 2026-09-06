//! **The three gate properties that fail silently, asserted where they are
//! declared** (WP-25 #36; C9, C14, C58, C24, M15).
//!
//! #36 adds three rules to the start gate and every one of them is invisible when it
//! is wrong. A fleet that starts on a logged-out harness looks like a fleet that
//! started — until five panes sit on a login prompt. A cost line that says "it will
//! spend tokens" and not *whose* tokens reads as a complete warning right up to the
//! invoice. A remembered model the vendor retired renders as a perfectly ordinary
//! row and dies at spawn. None of the three fails a compile, and the two halves of
//! each seam type-check independently.
//!
//! ## Why a Rust test for a TypeScript property
//!
//! The move `tests/views.rs`, `tests/gate_pickers.rs` (#35) and
//! `tests/placement_reads_nothing.rs` (#34) all make, for the reason C24 states: the
//! frontend has no test runner and this arc does not add one. Introducing a
//! JavaScript test framework as a side effect of a refusal rule is how a small
//! package becomes the polluted one. So this reads source.
//!
//! It reads the *declarations*, not the behaviour. The behaviour is asserted by the
//! unit tests in `src-tauri/src/fleet.rs` — `a_seat_that_cannot_log_in_refuses_the_start`,
//! `the_cost_line_says_whose_credential_each_seat_spends`, and
//! `a_model_the_catalog_no_longer_lists_falls_back_instead_of_spawning` — which run
//! the real functions against a machine a value describes. What this file adds is the
//! half those cannot reach: that the interface is wired to the same answers, and that
//! nothing on either side quietly grew a second implementation of one rule.
//!
//! ## The negative control
//!
//! Every check below is a function over a `&str`, and
//! [`the_checks_fire_on_a_source_that_violates_them`] runs each one against a
//! synthetic source that breaks exactly the property its test asserts — **and**
//! against the real source, to prove it is silent there. A check that reports
//! nothing is decoration; a check that reports everything is worse, because it looks
//! like the first one until somebody deletes the rule it was supposed to be guarding.
//! #35's own first draft had four of both kinds.

use std::path::{Path, PathBuf};

/// The interface's own declarations, which are what this file reads.
const GATE: &str = "ui/src/components/StartGate.tsx";
const WIRE: &str = "ui/src/fleet/types.ts";
const API: &str = "ui/src/fleet/api.ts";

/// The backend halves each of those must agree with.
const BACKEND: &str = "src-tauri/src/fleet.rs";
const HARNESS: &str = "src-tauri/src/placement/harness.rs";

/// The plan tiers C14 would have named and C58 narrowed away from (#34 measured that
/// the vendor's diagnostic reports the auth *shape* and not the tier). None of them
/// may be promised by a cost sentence this codebase writes.
const TIERS: [&str; 5] = ["Pro", "Max", "Plus", "Team", "Enterprise"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(rel: &str) -> String {
    let file = repo_root().join(rel);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()))
}

/// `text` with its comments removed.
///
/// **Comments have to be able to name their own subject**, which is the exemption
/// `gate_pickers.rs` and `placement_reads_nothing.rs` both state. It matters more
/// here than in either: the doc comment on `worker_cost` explains *why* the
/// orchestrator's credential is not the fleet's, so it necessarily spells both
/// halves of the split the code below must keep apart.
fn production_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("*/") {
            Some(close) => rest = &after[close + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.lines()
        .map(|line| match line.find("//") {
            // `https://` inside a string is not a comment, and neither is a `//`
            // that follows a colon for the same reason.
            Some(at) if !line[..at].ends_with(':') => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every double-quoted literal in `text`, in order.
fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}

/// The section of `text` between `after` and the next `until`, panicking with the
/// declaration's name if the shape it depends on is gone — a renamed declaration
/// must fail this test rather than quietly match nothing and pass.
fn between(text: &str, after: &str, until: &str, what: &str) -> String {
    let tail = text
        .split(after)
        .nth(1)
        .unwrap_or_else(|| panic!("{what} no longer starts with `{after}` — this file reads it"));
    tail.split(until)
        .next()
        .unwrap_or_else(|| panic!("{what} has no closing `{until}`"))
        .to_string()
}

/// Whether `word` appears in `text` as a word rather than inside a longer one.
///
/// `Pro` is a plan tier and `Provider` is not, and a check that could not tell them
/// apart would fire on `AccountShape::CustomProvider` forever and be deleted.
fn contains_word(text: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let boundary = |c: Option<char>| !matches!(c, Some(c) if c.is_alphanumeric() || c == '_');
        if boundary(before) && boundary(after) {
            return true;
        }
        from = end;
    }
    false
}

// --- the gate answers before there is a fleet ---------------------------------

/// The body of one Rust function, comments stripped.
fn rust_fn(backend: &str, signature: &str) -> String {
    between(&production_text(backend), signature, "\n}", signature)
}

/// Whether this command refuses to answer unless a fleet is already running.
fn requires_a_running_fleet(body: &str) -> bool {
    body.contains("no fleet is running")
}

/// **The gate answers before `fleet_bootstrap`, because that is when it runs.**
///
/// The one defect this ticket inherited, and the reason it is a test rather than a
/// line in the commit message: #35 hung the gate's state on `Fleet`, so `fleet_gate`
/// could only ever answer `no fleet is running` — the harness rows read "unavailable"
/// on every render, in the only state the pickers exist for. It type-checked on both
/// sides, no test failed, and the screen looked like a backend that was still
/// starting up.
///
/// Every property below it depends on this one: a refusal computed from a gate that
/// never has data is a refusal nobody can reach.
#[test]
fn the_gate_answers_without_a_fleet_because_that_is_when_it_runs() {
    let backend = read(BACKEND);
    for command in ["pub fn fleet_gate(", "pub fn fleet_set_seats("] {
        let body = rust_fn(&backend, command);
        assert!(
            !requires_a_running_fleet(&body),
            "`{command}` refuses unless a fleet is running. The start gate is the screen that \
             decides whether to start one, so this makes the pickers unreachable in the only \
             state they exist for:\n{body}",
        );
    }
    assert!(
        production_text(&backend).contains("gate: State<'_, Arc<GateHold>>"),
        "the gate's commands no longer take the app-managed `GateHold`, so whatever they read \
         belongs to something that does not exist yet",
    );
}

// --- refuse to start (story 11) -----------------------------------------------

/// Whether the start refusal is decided before the run boundary is cut.
///
/// `None` when one of the two is missing, which is itself a failure: a bootstrap
/// that no longer refuses, or one that no longer rotates, is not a bootstrap this
/// check understands.
fn refusal_precedes_the_run_boundary(bootstrap: &str) -> Option<bool> {
    let refuse = bootstrap.find("why_it_will_not_start")?;
    let rotate = bootstrap.find("runs::rotate")?;
    Some(refuse < rotate)
}

/// **The fleet refuses to start, before it has started anything** (story 11).
///
/// Two halves. It refuses *at all* — `spawn_pane` would refuse one pane at a time,
/// after the operator has been told the fleet started, which is the failure #35's
/// own doc comment names. And it refuses *first*: a refusal that has already
/// archived the last run and opened an empty database is not a refusal, it is a
/// start that failed and left a run behind.
#[test]
fn the_fleet_refuses_before_it_cuts_the_run_boundary() {
    let bootstrap = rust_fn(&read(BACKEND), "pub fn fleet_bootstrap(");
    assert_eq!(
        refusal_precedes_the_run_boundary(&bootstrap),
        Some(true),
        "`fleet_bootstrap` either stopped refusing a fleet that cannot log in, or it now cuts \
         the run boundary first — which archives the previous run and opens an empty database \
         on a start that is about to be refused:\n{bootstrap}",
    );
}

/// Whether the Start button is disabled on the backend's refusal.
fn start_is_disabled_on_a_refusal(gate: &str) -> bool {
    let button = between(gate, "className=\"pane-gate__go\"", "</button>", "the Start button");
    button.contains("disabled={refusals")
}

/// **The operator cannot press it, and can see why.**
///
/// Disabled rather than hidden, for user story 9's reason one screen over: a button
/// that vanished gives an operator no way to tell a refusal from a bug. The refusals
/// are also written out above the actions, because a `title` is not a message
/// somebody reads before they reach for the control.
#[test]
fn the_start_button_is_disabled_while_a_seat_cannot_take_one() {
    let gate = production_text(&read(GATE));
    assert!(
        start_is_disabled_on_a_refusal(&gate),
        "the Start button is not disabled on the gate's refusals. The backend still refuses, \
         so what this costs is an operator who clicks a live-looking button and gets an error \
         where a fleet should be",
    );
    assert!(
        gate.contains("verdict.refusals"),
        "the gate no longer renders the refusals themselves — a disabled button with no \
         sentence beside it is a supported feature looking unimplemented",
    );
}

/// The `LoginState` variants the backend will not seat, from the predicate itself.
fn variants_that_cannot_take_a_seat(harness: &str) -> Vec<String> {
    let body = between(
        &production_text(harness),
        "pub fn can_take_a_seat(&self) -> bool {",
        "\n    }",
        "can_take_a_seat",
    );
    body.split("LoginState::")
        .skip(1)
        .map(|rest| rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
        .collect()
}

/// Each `LoginState` variant paired with the status word the interface sees, read
/// out of the one match that decides them.
fn variant_status_words(backend: &str) -> Vec<(String, String)> {
    let body = between(
        &production_text(backend),
        "match &readiness.login {",
        "\n        };",
        "HarnessOffer::from",
    );
    body.split("LoginState::")
        .skip(1)
        .filter_map(|arm| {
            let variant: String =
                arm.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            quoted(arm).into_iter().next().map(|word| (variant, word))
        })
        .collect()
}

/// The status words the interface refuses a seat on.
fn words_the_interface_refuses(wire: &str) -> Vec<String> {
    quoted(&between(
        &production_text(wire),
        "export const CANNOT_TAKE_A_SEAT",
        ";",
        "CANNOT_TAKE_A_SEAT",
    ))
}

/// **One rule, read from both ends.**
///
/// `LoginState::can_take_a_seat` decides whether the fleet starts; `CANNOT_TAKE_A_SEAT`
/// decides whether the row is greyed out. If they part company the gate offers a seat
/// the fleet then refuses, or greys out one it would have started — and both look
/// like the harness is broken rather than the rule.
///
/// The pairing goes through `HarnessOffer::from`, which is the one place a variant
/// becomes a word, so this cannot be satisfied by two lists that happen to be the
/// same length.
#[test]
fn what_the_backend_refuses_is_what_the_interface_greys_out() {
    let variants = variants_that_cannot_take_a_seat(&read(HARNESS));
    assert_eq!(
        variants.len(),
        2,
        "`can_take_a_seat` no longer names two refusing states ({variants:?}). C14 keeps the \
         refusal narrow — `Unreadable` is a working installation this build could not parse — \
         so a third is a decision, not a typo",
    );
    let words = variant_status_words(&read(BACKEND));
    let mut refused: Vec<String> = variants
        .iter()
        .map(|variant| {
            words
                .iter()
                .find(|(name, _)| name == variant)
                .unwrap_or_else(|| panic!("`HarnessOffer::from` has no arm for {variant}"))
                .1
                .clone()
        })
        .collect();
    let mut interface = words_the_interface_refuses(&read(WIRE));
    refused.sort();
    interface.sort();
    assert_eq!(
        refused, interface,
        "the backend refuses a seat in {refused:?} and the interface greys one out in \
         {interface:?}. A state on one list and not the other is a gate that offers what the \
         fleet will not start, or refuses what it would have",
    );
}

// --- the cost line, and C9's split (story 12) ---------------------------------

/// Which credential a sentence says a seat **runs on**.
///
/// The verb is load-bearing and the first draft of this check did not have it: the
/// worker's sentence ends "never your own login or plan", which *mentions* the
/// operator's login in order to rule it out. Matching the mention flagged a sentence
/// that was saying exactly the right thing. What the two halves of C9 assert is the
/// claim — which credential the seat is described as running on — so the check reads
/// the verb with it.
fn names_the_operators_own_login(sentence: &str) -> bool {
    sentence.contains("runs your own login")
}

fn names_the_fleets_own_key(sentence: &str) -> bool {
    sentence.contains("runs FLEETOR's own provider and key")
}

/// Whether a worker sentence describes running the operator's plan.
///
/// **A separate predicate from [`names_the_operators_own_login`], and not a
/// widening of it.** That one reads the orchestrator's fixed sentence; this one
/// reads a branch. Sharing a matcher between them is what let this file keep
/// passing after C75 moved the axis: `worker_cost`'s plan branch says "runs your
/// own `{harness}` login", which the orchestrator's matcher does not see, so the
/// old assertion went on being true about a sentence it was no longer about.
fn names_the_operators_plan(sentence: &str) -> bool {
    sentence.contains("runs your own {harness} login")
}

/// **The cost line's split is plan-versus-fleet-key** (story 12, reshaped by C75).
///
/// **This assertion used to be orchestrator-versus-worker, and #49 predicted it
/// would go wrong here rather than fail.** It was true while a worker could only
/// run the fleet's key; C73 made a worker able to run the operator's plan, so a
/// test pinning "a worker's sentence never names the operator's login" is pinning
/// the absence of the feature. It is reshaped rather than deleted, because the
/// property underneath it is the one that matters and has not changed: **a cost
/// sentence must name the credential the seat actually runs on, and no sentence
/// may name the wrong one.**
///
/// So: the orchestrator's line is unchanged and still says the operator's own
/// login. The worker's function must carry **both** branches — a fleet-key
/// sentence and a plan sentence — because a `worker_cost` with only one of them is
/// a gate that lies to half the fleets it renders.
#[test]
fn the_cost_sentences_name_the_credential_each_seat_runs_on() {
    let backend = read(BACKEND);
    let orch = rust_fn(&backend, "fn orchestrator_cost(");
    let worker = rust_fn(&backend, "fn worker_cost(");

    assert!(
        names_the_operators_own_login(&orch),
        "the orchestrator's cost line no longer says whose login it runs. \"It will spend \
         tokens\" is not the warning when the tokens are the operator's own plan:\n{orch}",
    );
    assert!(
        !names_the_fleets_own_key(&orch),
        "the orchestrator's cost line claims FLEETOR's provider and key. It runs the \
         operator's own login (C9) — a gate that said otherwise would tell somebody their \
         subscription is safe while it is being spent:\n{orch}",
    );
    assert!(
        names_the_fleets_own_key(&worker),
        "a worker's cost line no longer says it runs FLEETOR's own provider and key. That \
         is still what a fleet-key seat does, and it is still the majority of \
         them:\n{worker}",
    );
    assert!(
        names_the_operators_plan(&worker),
        "a worker's cost line has no plan branch, so a fleet whose seats spend the \
         operator's subscription is described as spending FLEETOR's metered key (C75). \
         That is the wrong direction to be wrong in:\n{worker}",
    );
    assert!(
        worker.contains("no per-pane budget"),
        "the plan branch stops saying that FLEETOR caps nothing. No per-pane budget \
         exists anywhere in this codebase, and four uncapped seats on one subscription is \
         the cost the operator is agreeing to:\n{worker}",
    );
}

/// **The plan is a choice the operator makes, and it is stated before Start**
/// (#49, C75).
///
/// Two properties in one test because they are one requirement: a fleet may spend
/// the operator's subscription, and the operator must have said so and been told
/// how many seats will do it. Either half alone is the failure — a picker with no
/// count is a choice made blind, and a count with no picker is an announcement.
#[test]
fn the_gate_says_how_many_seats_will_spend_the_operators_plan() {
    let backend = read(BACKEND);
    let verdict = rust_fn(&backend, "    fn for_seats(");

    assert!(
        verdict.contains("plan_seats"),
        "the start verdict no longer counts the seats about to spend the operator's \
         plan:\n{verdict}",
    );
    assert!(
        verdict.contains("gap_for"),
        "the gate no longer refuses a seat whose credential this machine cannot honour. \
         Falling back silently bills the operator for a choice they did not make in one \
         direction, and spends their subscription in the other:\n{verdict}",
    );

    // **A seat that could run on the other credential is moved, not refused**
    // (C79 narrowed C78 here). The strict rule met a dead gate on arrival: the
    // default is the plan on every seat, so a machine where one harness has no
    // login refused a fleet the operator had not configured at all. What survives
    // is the property that actually mattered — no *invisible* spend — which the
    // move satisfies by being announced.
    let settle = rust_fn(&backend, "fn settle_credentials(");
    assert!(
        settle.contains("gap_for") && settle.contains("CredentialFallback"),
        "seats are no longer moved onto the credential this machine has, so a fleet nobody \
         configured can arrive already refused:\n{settle}",
    );
    assert!(
        settle.contains("seat.model = None"),
        "a moved seat keeps its old model, which the credential it moved to has never heard \
         of:\n{settle}",
    );

    // **The refusal that is left is the one with nowhere to go**, and it must name
    // both fixes — either one alone unblocks the seat.
    let refusal = rust_fn(&backend, "fn credential_gap(");
    assert!(
        refusal.contains("Re-check logins"),
        "the refusal no longer tells the operator how to log in — one of its two \
         actionable lines:\n{refusal}",
    );
    assert!(
        refusal.contains("DEEPSEEK_API_KEY"),
        "the refusal no longer names the key that would unblock the seat, so an operator \
         with no login is told only about the fix they cannot take:\n{refusal}",
    );
    assert!(
        refusal.contains("neither"),
        "the refusal no longer says both credentials are missing. A seat short of only one \
         is moved rather than refused now, so a refusal that reads as a single gap sends \
         the operator to fix something that was never the problem:\n{refusal}",
    );
}

/// Any plan tier a stretch of text promises.
fn tiers_promised(text: &str) -> Vec<String> {
    TIERS.iter().filter(|tier| contains_word(text, tier)).map(|tier| (*tier).to_string()).collect()
}

/// **No cost sentence promises a plan tier** (C14 as narrowed by C58).
///
/// C14 said the gate would show "ChatGPT &lt;plan&gt;". #34 measured that the
/// vendor's diagnostic reports the auth *shape* and not the tier — the tier lives
/// inside the stored id token, and reading it would mean parsing the credential file
/// this arc took pains to reach only through the vendor. So the shape is what the
/// sentences carry, by way of `AccountShape::display`, and a tier can appear only if
/// a vendor reports one. `gate_pickers.rs` asserts the same thing about the wire
/// type's fields; this asserts it about the prose, which is where it would actually
/// reach a person.
#[test]
fn no_cost_sentence_promises_a_plan_tier() {
    let backend = read(BACKEND);
    let sentences =
        format!("{}{}", rust_fn(&backend, "fn orchestrator_cost("), rust_fn(&backend, "fn worker_cost("));
    let promised = tiers_promised(&sentences);
    assert!(
        promised.is_empty(),
        "a cost sentence names the plan tier{promised:?}. The vendor's diagnostic does not \
         report one (C58), so anything here is invented:\n{sentences}",
    );
}

// --- the stale model falls back rather than spawning (story 14) ---------------

/// Whether the seats are settled against the live catalog before they are stored.
fn settles_before_it_stores(answer: &str) -> Option<bool> {
    Some(answer.find("settle_models")? < answer.find("store_seats")?)
}

/// **A retired model falls back on the way in, not on the way out** (story 14).
///
/// The fallback is applied to the value that is *stored*, so the seat `spawn_pane`
/// later reads already carries the default. Settling after storing — or only for
/// display — would leave the retired id on the cell that places, which is the whole
/// failure: the row would say one thing and the `--model` flag would carry another.
#[test]
fn the_stale_model_falls_back_before_the_selection_is_stored() {
    let backend = read(BACKEND);
    let answer = rust_fn(&backend, "fn answer(");
    assert_eq!(
        settles_before_it_stores(&answer),
        Some(true),
        "the gate stores a selection before settling it against the live catalog, so a model \
         the vendor no longer lists reaches the cell `spawn_pane` places against:\n{answer}",
    );
    let bootstrap = rust_fn(&backend, "pub fn fleet_bootstrap(");
    assert_eq!(
        settles_before_it_stores(&bootstrap),
        Some(true),
        "the start path no longer settles the seats before storing them, so a fleet reached \
         without the gate can still spawn a retired model:\n{bootstrap}",
    );
    let gate = production_text(&read(GATE));
    assert!(
        gate.contains("verdict.fallbacks"),
        "the gate no longer renders the fallbacks. A silent substitution is exactly what \
         story 14 refuses: the operator asked for a model and got a different one",
    );
}

/// The sentinel as each side spells it.
fn sentinel_in_rust(backend: &str) -> Vec<String> {
    quoted(&between(&production_text(backend), "pub const DEFAULT_YOUR_LOGIN", ";", "the sentinel"))
}

fn sentinel_in_typescript(wire: &str) -> Vec<String> {
    quoted(&between(&production_text(wire), "export const DEFAULT_YOUR_LOGIN", ";", "the sentinel"))
}

/// **What a fallback falls back *to* is spelled once.**
///
/// The notice names the seat's own default in words, and the row's placeholder shows
/// the same words. Two spellings would put the operator in front of a sentence
/// telling them a model became something their screen does not show.
#[test]
fn the_fallback_lands_on_the_sentinel_both_sides_spell() {
    let rust = sentinel_in_rust(&read(BACKEND));
    let typescript = sentinel_in_typescript(&read(WIRE));
    assert_eq!(
        rust, typescript,
        "the orchestrator's sentinel is {rust:?} in Rust and {typescript:?} in the interface. \
         A fallback notice would name a default the row does not show",
    );
    assert_eq!(rust.len(), 1, "the sentinel is one string, not {rust:?}");
}

// --- one answer, read rather than re-derived (M15) ----------------------------

/// Whether the command answers with the whole gate rather than the seats alone.
fn answers_with_the_whole_gate(backend: &str, api: &str) -> bool {
    let signature = between(
        &production_text(backend),
        "pub fn fleet_set_seats(",
        "{",
        "fleet_set_seats' signature",
    );
    signature.contains("Result<GateState, String>") && api.contains("Promise<GateState>")
}

/// **The summary reads the state the pickers write, and derives nothing** (M15).
///
/// `fleet_set_seats` answers with the whole `GateState`, so a pick replaces the whole
/// screen with the backend's own reading of the fleet it just stored — the refusal,
/// the cost lines and the fallbacks included. The alternative was returning the seats
/// and re-deriving the rest here, which is two implementations of one rule with
/// nothing pinning them together; the one that is wrong is the one the operator reads.
#[test]
fn the_summary_is_the_backends_answer_and_not_a_second_derivation() {
    let backend = read(BACKEND);
    let api = read(API);
    assert!(
        answers_with_the_whole_gate(&backend, &api),
        "`fleet_set_seats` no longer answers with the whole gate, so the interface has to \
         work out for itself what the fleet it just stored will cost and whether it may start",
    );
    let gate = production_text(&read(GATE));
    assert!(
        gate.contains("gate?.verdict.refusals") || gate.contains("gate.verdict.refusals"),
        "the gate computes its own refusals instead of reading the backend's",
    );
    for rendered in ["verdict.cost", "verdict.fallbacks", "verdict.refusals"] {
        assert!(gate.contains(rendered), "the gate no longer renders `{rendered}`");
    }
}

// --- the caveat keeps its one wording -----------------------------------------

/// Any interface file that spells the reachability caveat itself rather than
/// rendering the one the backend sends.
fn respells_the_caveat(text: &str, fragment: &str) -> bool {
    production_text(text).contains(fragment)
}

/// **`doctor` reports reachability, not authorization — said once** (#34, C58).
///
/// A revoked key clears the gate and fails on the pane's first turn, and the fleet
/// declines to spend a token to find out. #35 renders that sentence once per distinct
/// harness the fleet is about to spend, from `REACHABILITY_NOT_AUTHORIZATION`. #36
/// adds three more operator-facing sentences to the same card, and the temptation
/// each time is to restate the caveat beside them — at which point there are two
/// wordings of one fact and they drift.
#[test]
fn the_reachability_caveat_is_still_the_backends_one_wording() {
    let fragment = "revoked or expired key clears this check";
    assert!(
        read(HARNESS).contains(fragment),
        "`REACHABILITY_NOT_AUTHORIZATION` no longer says what a passing check does not prove — \
         this file reads that sentence",
    );
    for rel in [GATE, WIRE, API] {
        assert!(
            !respells_the_caveat(&read(rel), fragment),
            "{rel} spells the reachability caveat itself. It is the backend's sentence, \
             rendered from `offer.caveat`, and a second wording of it will drift",
        );
    }
    assert!(
        production_text(&read(GATE)).contains("offer.caveat"),
        "the gate no longer renders the caveat at all",
    );
}

// --- the negative control -----------------------------------------------------

/// **Each check fires on a source that violates it, and is silent on the real one.**
///
/// Both halves, because they fail in opposite directions and only one of them is
/// obvious. A check that never fires is decoration. A check that always fires looks
/// identical to a working one until the day somebody deletes the rule and it reports
/// the same thing it always did — which is the half people forget, and the half #35
/// added after its own first draft had two of them.
#[test]
fn the_checks_fire_on_a_source_that_violates_them() {
    // A gate command that refuses to answer without a fleet — the defect this ticket
    // inherited, written out.
    let needy = "let fleet = guard.as_ref().ok_or(\"no fleet is running\")?;";
    assert!(requires_a_running_fleet(needy), "the pre-bootstrap check would not notice a refusal");
    let backend = read(BACKEND);
    for command in ["pub fn fleet_gate(", "pub fn fleet_set_seats("] {
        assert!(
            !requires_a_running_fleet(&rust_fn(&backend, command)),
            "the pre-bootstrap check reports `{command}` itself, so it says nothing about a \
             command that does require a fleet",
        );
    }

    // A bootstrap that rotates the run before deciding whether to refuse it.
    let backwards = "let rotation = runs::rotate(&dir);\nif let Some(why) = \
                     verdict.why_it_will_not_start() { return Err(why); }";
    assert_eq!(
        refusal_precedes_the_run_boundary(backwards),
        Some(false),
        "the ordering check would not notice a refusal that comes after the run boundary",
    );
    assert_eq!(
        refusal_precedes_the_run_boundary("let rotation = runs::rotate(&dir);"),
        None,
        "the ordering check claims an answer for a bootstrap that no longer refuses at all",
    );
    assert_eq!(
        refusal_precedes_the_run_boundary(&rust_fn(&backend, "pub fn fleet_bootstrap(")),
        Some(true),
        "the ordering check reports the real bootstrap",
    );

    // A Start button with nothing stopping it.
    let live = "className=\"pane-gate__go\" onClick={onStart}>Start fleet</button>";
    assert!(
        !start_is_disabled_on_a_refusal(live),
        "the disabled-button check would not notice a button that is always live",
    );
    assert!(
        start_is_disabled_on_a_refusal(&production_text(&read(GATE))),
        "the disabled-button check does not find the real button, so it is asserting nothing",
    );

    // A third refusing state on one side of the wire only.
    let widened = "pub fn can_take_a_seat(&self) -> bool {\n        !matches!(self, \
                   LoginState::NoCredential { .. } | LoginState::NotInstalled | \
                   LoginState::Unreadable { .. })\n    }";
    assert_eq!(
        variants_that_cannot_take_a_seat(widened).len(),
        3,
        "the refusing-variant scan does not read the predicate it is pointed at",
    );
    assert_eq!(
        variants_that_cannot_take_a_seat(&read(HARNESS)),
        vec!["NoCredential".to_string(), "NotInstalled".to_string()],
        "the refusing-variant scan does not read the real predicate",
    );
    assert_eq!(
        words_the_interface_refuses(
            "export const CANNOT_TAKE_A_SEAT: readonly HarnessStatus[] = [\"no-credential\"];"
        ),
        vec!["no-credential".to_string()],
        "the interface-refusal scan does not read the list it is pointed at",
    );

    // A cost line that swapped the two credentials over — C9 inverted.
    let swapped =
        "format!(\"{harness} in the orchestrator seat runs FLEETOR's own provider and key.\")";
    assert!(
        names_the_fleets_own_key(swapped) && !names_the_operators_own_login(swapped),
        "the C9 split check would not notice an orchestrator line claiming the fleet's key",
    );
    assert!(
        names_the_operators_own_login(&rust_fn(&backend, "fn orchestrator_cost("))
            && !names_the_fleets_own_key(&rust_fn(&backend, "fn orchestrator_cost(")),
        "the C9 split check does not read the real orchestrator sentence",
    );
    assert!(
        names_the_fleets_own_key(&rust_fn(&backend, "fn worker_cost("))
            && !names_the_operators_own_login(&rust_fn(&backend, "fn worker_cost(")),
        "the C9 split check does not read the real worker sentence",
    );

    // A tier promised, and a provider that only looks like one.
    assert_eq!(
        tiers_promised("runs your own login (ChatGPT Pro) on openai"),
        vec!["Pro".to_string()],
        "the tier check would not notice a promised plan tier",
    );
    assert!(
        tiers_promised("AccountShape::CustomProvider { name, .. } => format!(\"{name}\")")
            .is_empty(),
        "the tier check fires on `CustomProvider`, which contains `Pro` and is not a tier",
    );

    // A settle that runs after the store, or not at all.
    assert_eq!(
        settles_before_it_stores("let seats = gate.store_seats(picked); settle_models(seats)"),
        Some(false),
        "the settle-order check would not notice a fallback applied after the store",
    );
    assert_eq!(
        settles_before_it_stores("let seats = gate.store_seats(picked);"),
        None,
        "the settle-order check claims an answer for a path that no longer settles",
    );
    assert_eq!(
        settles_before_it_stores(&rust_fn(&backend, "fn answer(")),
        Some(true),
        "the settle-order check does not read the real assembly",
    );

    // Two spellings of the sentinel.
    assert_ne!(
        sentinel_in_rust("pub const DEFAULT_YOUR_LOGIN: &str = \"the vendor's default\";"),
        sentinel_in_typescript("export const DEFAULT_YOUR_LOGIN = \"default (your login)\";"),
        "the sentinel check does not notice two spellings",
    );
    assert_eq!(
        sentinel_in_rust(&backend),
        sentinel_in_typescript(&read(WIRE)),
        "the sentinel check reports the real pair, which agree",
    );

    // A `fleet_set_seats` that answers with the seats alone.
    assert!(
        !answers_with_the_whole_gate(
            "pub fn fleet_set_seats(seats: FleetSeats) -> Result<FleetSeats, String> {",
            "Promise<FleetSeats>",
        ),
        "the whole-gate check would not notice a command answering with the seats alone",
    );
    assert!(
        answers_with_the_whole_gate(&backend, &read(API)),
        "the whole-gate check does not read the real command",
    );

    // The caveat, respelled in the interface.
    let respelt = "a revoked or expired key clears this check and fails on turn one";
    assert!(
        respells_the_caveat(respelt, "revoked or expired key clears this check"),
        "the caveat check would not notice a second wording in the interface",
    );
    for rel in [GATE, WIRE, API] {
        assert!(
            !respells_the_caveat(&read(rel), "revoked or expired key clears this check"),
            "the caveat check reports {rel}, so it says nothing about a file that does respell it",
        );
    }
}

/// **The declarations this file reads are still there**, so a rename fails loudly
/// rather than leaving every test above matching nothing.
#[test]
fn the_declarations_this_file_reads_still_exist() {
    for rel in [GATE, WIRE, API, BACKEND, HARNESS] {
        let file = repo_root().join(rel);
        assert!(file.is_file(), "{} is read by this file and is gone", file.display());
    }
    // The three properties are three declarations; a rename of any of them must
    // arrive here as a panic from `between`, not as a check that quietly matches
    // nothing.
    let backend = read(BACKEND);
    for signature in [
        "fn orchestrator_cost(",
        "fn worker_cost(",
        "fn settle_models(",
        "fn answer(",
        "pub fn fleet_bootstrap(",
        "pub fn fleet_gate(",
        "pub fn fleet_set_seats(",
    ] {
        assert!(backend.contains(signature), "`{signature}` is read by this file and is gone");
    }
    assert!(
        production_text(&backend).contains("fn why_it_will_not_start(&self)"),
        "`StartVerdict::why_it_will_not_start` is what both refusal sites call, and is gone",
    );
}
