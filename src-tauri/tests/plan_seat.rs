//! **A worker seat runs on the operator's own login** (WP-25 #49; C73, C75, D-118).
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

    let placed = bench.place(PaneSpec::worker(1, harness::claude_code()));
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

    // `ANTHROPIC_AUTH_TOKEN`'s mere presence selects the metered path, and a base
    // URL points the login at another provider (C71).
    assert_eq!(env_on(&placed.command, "ANTHROPIC_BASE_URL"), None);
    assert_eq!(
        env_on(&placed.command, "ANTHROPIC_AUTH_TOKEN"),
        None,
        "a plan seat carries a token variable, which selects metered billing",
    );
}

/// **Codex needed no spike and still needs the same wiring** (C6, C75).
///
/// Its login is a file inside the directory this fleet replaces, so the mechanism
/// is the same write to a different name.
#[test]
fn a_codex_plan_seat_gets_auth_json() {
    let mut bench = Bench::new("plan-seat-codex");
    bench.operator_logs_in(codex(), DOCUMENT);

    bench.place(PaneSpec::worker(1, fleetor_shell::placement::codex::codex()));
    let dir = bench.config_dir(PaneId::Worker(1));

    let auth = dir.join("auth.json");
    assert!(auth.is_file(), "the codex plan seat has no `auth.json`, so it will spawn logged out");
    assert_eq!(std::fs::read_to_string(&auth).expect("read"), DOCUMENT);
}

/// **A worker seat on a machine with no readable login refuses** (C75).
///
/// Proceeding without a credential produces D-062's pane — one that reaches its
/// prompt, reports `accepted` on every `fleet send`, and is logged out.
#[test]
fn a_worker_seat_with_no_login_refuses_and_names_the_command() {
    let mut bench = Bench::new("plan-seat-no-login");
    bench.host.operator_logins.clear();

    let refused = bench
        .try_place(PaneSpec::worker(1, harness::claude_code()))
        .expect_err("a worker seat with no login must not place");

    assert!(
        refused.contains("claude"),
        "the refusal does not name the command that would fix it:\n{refused}",
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
/// FLEETOR asserts nothing about a provider it does not own, and on a worker seat
/// the provider is the vendor's.
#[test]
fn a_plan_seat_is_told_no_model_and_no_window() {
    let mut bench = Bench::new("plan-seat-model");
    bench.operator_logs_in(claude_code(), DOCUMENT);

    let placed = bench.place(PaneSpec::worker(1, harness::claude_code()));

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
