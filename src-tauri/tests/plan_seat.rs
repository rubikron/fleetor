//! **A worker seat that spends the operator's own plan** (WP-25 #49; C73, C75).
//!
//! The feature this file pins is a reversal of D-062's original rule, and the
//! reversal is deliberate: a fenced worker used to hold the fleet's credential and
//! never the operator's, and now it may hold theirs if the operator said so at the
//! gate. That makes **the difference between the two kinds of seat** the property
//! worth asserting — not the absence of a credential, which is now only half true.
//!
//! **This is where #29's byte-scan gets scoped rather than weakened.** That scan
//! asks "does a fenced worktree contain the operator's credential", and on a
//! fleet-key seat the answer must still be no. On a plan seat the answer is *yes,
//! and that is the point*, so a test that could not tell the seats apart would have
//! to be either deleted or made to pass by accident. Both kinds are placed here,
//! from the same bench, and asserted against each other.
//!
//! **No real credential is used.** `OperatorLogin` is opaque to everything a
//! placement reaches, so a stand-in document proves exactly what a real one would:
//! where the bytes land, at what mode, and which environment variables are absent.

mod common;

use common::Bench;
use fleetor_core::pane::PaneId;
use fleetor_shell::placement::{harness, PaneSpec};

/// The stand-in credential. Distinctive so a scan can find it anywhere it leaked.
const DOCUMENT: &str = r#"{"claudeAiOauth":{"accessToken":"stand-in-not-a-real-token"}}"#;

/// Claude Code's name, as `HarnessSpec::name` spells it. One spelling, and the
/// registry is what says it rather than this file guessing.
fn claude_code() -> &'static str {
    harness::claude_code().spec().name
}

fn codex() -> &'static str {
    fleetor_shell::placement::codex::codex().spec().name
}

fn env_on(command: &portable_pty::CommandBuilder, name: &str) -> Option<String> {
    command.get_env(name).map(|v| v.to_string_lossy().to_string())
}

/// **The whole mechanism, on the harness that needed a spike for it** (C73).
///
/// A plan seat is still fenced — private `HOME`, its own worktree, its own config
/// dir — and the credential arrives in that config dir because it is the one store
/// a fenced pane can still read. `plan-worker-notes.md` §12 is the measurement;
/// this is the wiring that acts on it.
#[test]
fn a_claude_code_plan_seat_gets_the_credential_in_its_own_config_dir() {
    let mut bench = Bench::new("plan-seat-cc");
    bench.operator_logs_in(claude_code(), DOCUMENT);

    let placed = bench.place(PaneSpec::worker(1, harness::claude_code()).on_the_operators_plan());
    let file = bench.config_dir(PaneId::Worker(1)).join(".credentials.json");

    assert!(file.is_file(), "the plan seat has no credential, so it will spawn logged out");
    assert_eq!(
        std::fs::read_to_string(&file).expect("read the planted credential"),
        DOCUMENT,
        "the credential was rewritten on its way in",
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = file.metadata().expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the operator's credential is world-readable on disk");
    }

    // **Neither fleet variable, and this is the half that is easy to get wrong.**
    // `ANTHROPIC_BASE_URL` would point a pane holding an Anthropic OAuth credential
    // at the fleet's DeepSeek endpoint; `ANTHROPIC_AUTH_TOKEN`'s mere presence
    // selects the metered path, so setting it would put the seat on API billing
    // while the gate promised the operator's plan (C71).
    assert_eq!(
        env_on(&placed.command, "ANTHROPIC_BASE_URL"),
        None,
        "a plan seat was pointed at the fleet's endpoint",
    );
    assert_eq!(
        env_on(&placed.command, "ANTHROPIC_AUTH_TOKEN"),
        None,
        "a plan seat carries the fleet's token variable, which selects metered billing",
    );
}

/// **Codex needed no spike and still needs the same wiring** (C6, C75).
///
/// Its login is a file inside the directory this fleet replaces, so the mechanism
/// is the same write to a different name — and the FLEETOR provider row, which
/// points a worker at the fleet's endpoint, must not be written on this seat.
#[test]
fn a_codex_plan_seat_gets_auth_json_and_not_the_fleets_provider() {
    let mut bench = Bench::new("plan-seat-codex");
    bench.operator_logs_in(codex(), DOCUMENT);

    bench.place(PaneSpec::worker(1, fleetor_shell::placement::codex::codex()).on_the_operators_plan());
    let dir = bench.config_dir(PaneId::Worker(1));

    let auth = dir.join("auth.json");
    assert!(auth.is_file(), "the codex plan seat has no `auth.json`, so it will spawn logged out");
    assert_eq!(std::fs::read_to_string(&auth).expect("read"), DOCUMENT);

    let config = std::fs::read_to_string(dir.join("config.toml")).expect("the seeded document");
    assert!(
        !config.contains("model_provider = \"fleetor\"")
            && !config.contains("model_provider=\"fleetor\""),
        "a plan seat was pointed at the FLEETOR provider, which is a different vendor at a \
         different endpoint — the operator's login authenticates nothing there:\n{config}",
    );
}

/// **#29's byte-scan, scoped rather than weakened** (C75).
///
/// The property it defends is unchanged on the seat it was written for: a fleet-key
/// worker's fenced directories contain no byte of the operator's credential, and it
/// runs on the fleet's endpoint and key. Asserting it *beside* the plan seat is
/// what makes the scoping visible — a reader who changes one sees the other.
#[test]
fn a_fleet_key_seat_still_holds_nothing_of_the_operators() {
    let mut bench = Bench::new("plan-seat-fleet-key");
    // The machine *has* a login. The seat still must not get it, because this seat
    // did not ask for it — which is the case a scan on a logged-out machine could
    // never have caught.
    bench.operator_logs_in(claude_code(), DOCUMENT);

    let placed = bench.place(PaneSpec::worker(1, harness::claude_code()));
    let dir = bench.config_dir(PaneId::Worker(1));

    assert!(
        !dir.join(".credentials.json").exists(),
        "a fleet-key seat was handed the operator's credential",
    );
    for entry in std::fs::read_dir(&dir).expect("read the pane's config dir").flatten() {
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            assert!(
                !text.contains("stand-in-not-a-real-token"),
                "the operator's credential leaked into {}",
                entry.path().display(),
            );
        }
    }

    assert!(
        env_on(&placed.command, "ANTHROPIC_AUTH_TOKEN").is_some(),
        "a fleet-key seat lost the fleet's own key",
    );
    assert!(
        env_on(&placed.command, "ANTHROPIC_BASE_URL").is_some(),
        "a fleet-key seat lost the fleet's own endpoint",
    );
}

/// **A plan seat on a machine with no readable login refuses, rather than quietly
/// becoming a fleet-key seat** (C75).
///
/// Both silent alternatives are worse than a refusal. Falling back bills the
/// operator for a choice they did not make; proceeding without a credential
/// produces D-062's pane — one that reaches its prompt, reports `accepted` on every
/// `fleet send`, and is logged out.
#[test]
fn a_plan_seat_with_no_login_refuses_and_names_the_command() {
    let bench = Bench::new("plan-seat-no-login");

    let refused = bench
        .try_place(PaneSpec::worker(1, harness::claude_code()).on_the_operators_plan())
        .expect_err("a plan seat with no login must not place");

    assert!(
        refused.contains("claude"),
        "the refusal does not name the command that would fix it:\n{refused}",
    );
    assert!(
        refused.to_lowercase().contains("metered key"),
        "the refusal does not say FLEETOR declined to fall back, so an operator reads it as \
         a bug rather than as a choice being honoured:\n{refused}",
    );
    assert!(
        !bench.config_dir(PaneId::Worker(1)).join(".credentials.json").exists(),
        "the refusal still wrote a credential file",
    );
}

/// **The orchestrator is untouched by any of this** (C9, C75).
///
/// That seat has always run the operator's own login, through the keychain and not
/// through a planted file, and this feature must not have given it a second
/// credential channel by accident.
#[test]
fn the_orchestrator_gains_no_planted_credential() {
    let mut bench = Bench::new("plan-seat-orch");
    bench.operator_logs_in(claude_code(), DOCUMENT);

    bench.place(PaneSpec::orch(harness::claude_code()));

    assert!(
        !bench.config_dir(PaneId::Orch).join(".credentials.json").exists(),
        "the orchestrator was handed a planted credential. It reaches the operator's login \
         through the keychain (D-062), and a second channel is a second thing to revoke",
    );
}

/// **A plan seat names no model, and FLEETOR asserts no context window on it**
/// (C80) — the bug this test exists for, reported from a running fleet.
///
/// A claude-code worker set to the operator's plan came up on `deepseek-v4-flash`.
/// The gate was right — the row showed `default (your login)` — and the spawn path
/// substituted the fleet's model anyway, one frame before anything could tell the
/// two kinds of seat apart. `CLAUDE_CODE_MAX_CONTEXT_TOKENS` had the same shape:
/// exported unconditionally, so a pane running the operator's own model was told
/// to compact against a window belonging to a different provider.
///
/// **Both are the same rule read twice:** FLEETOR asserts facts about *its own*
/// provider, and on a plan seat the provider is the vendor's.
#[test]
fn a_plan_seat_is_told_no_model_and_no_window() {
    let mut bench = Bench::new("plan-seat-model");
    bench.operator_logs_in(claude_code(), DOCUMENT);

    let placed = bench.place(PaneSpec::worker(1, harness::claude_code()).on_the_operators_plan());

    assert_eq!(
        env_on(&placed.command, "ANTHROPIC_MODEL"),
        None,
        "a plan seat carries the fleet's model. Its provider is the vendor's own, which has \
         never heard of that name — and the gate promised the vendor's default",
    );
    assert_eq!(
        env_on(&placed.command, "CLAUDE_CODE_MAX_CONTEXT_TOKENS"),
        None,
        "a plan seat is told the fleet model's context window. It is running a different \
         model on a different provider, so this makes it compact early against a number \
         that is not its own (WP-02)",
    );
    assert_eq!(
        placed.model, None,
        "the placement reports a model for a seat that was given none, so the rail and the \
         gauge describe a pane that does not exist",
    );
}

/// **A fleet-key seat keeps both, unchanged** (C9).
///
/// The other half of the same rule, and the reason C80 is a narrowing rather than
/// a removal: a worker on FLEETOR's provider has no vendor default to fall back
/// to, so an unnamed model there is still the launch configuration's.
#[test]
fn a_fleet_key_seat_still_gets_the_fleets_model_and_window() {
    let bench = Bench::new("fleet-key-seat-model");

    let placed = bench.place(PaneSpec::worker(1, harness::claude_code()));

    assert!(
        env_on(&placed.command, "ANTHROPIC_MODEL").is_some(),
        "a fleet-key seat lost the fleet's model. Its provider is FLEETOR's and there is no \
         vendor default to fall back to (C9), so it would spawn naming nothing",
    );
    assert!(
        env_on(&placed.command, "CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_some(),
        "a fleet-key seat lost the exported context window, so it assumes its vendor's own \
         and auto-compacts early (WP-02)",
    );
}
