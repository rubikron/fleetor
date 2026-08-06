//! `fleet` — the whole agent-facing surface of a FLEETOR fleet (D-030).
//!
//! Six verbs, one unix socket, no MCP server. A pane's `claude` runs this
//! through its Bash tool; the hub writes the message straight into the target
//! pane's terminal and answers with what actually happened.
//!
//! Three properties this binary must keep, because a model reads its output and
//! acts on it:
//!
//!  1. **Non-zero means not delivered.** Both briefs promise it verbatim. A
//!     refused send exits 1 with the reason on stderr, so the model self-corrects
//!     instead of assuming its message landed.
//!  2. **Zero is not a promise the agent read it.** We say "accepted", never
//!     "delivered" (L3).
//!  3. **No timeout, anywhere.** [`Client::call`] waits as long as the hub takes.
//!     A ceiling here could only turn a slow delivery into a *reported failure for
//!     a message that still arrives* — the request is already in flight when a
//!     timer would fire, so it cannot cancel anything. It would make the model
//!     resend and make the log lie (D-034).

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fleetor_core::pane::{PaneEntry, PaneId};
use fleetor_core::wire::{Hello, Op, OpResult};
use fleetor_ipc::{Client, UnixTransport};
use std::path::PathBuf;
use std::process::ExitCode;

/// Set by the spawn path on every pane. Without it the hub cannot attribute a
/// message, so every verb but `whoami` refuses rather than guessing.
const ENV_PANE: &str = "FLEETOR_PANE";
/// Set by the spawn path; the hub's unix socket.
const ENV_SOCKET: &str = "FLEET_SOCKET";
/// What a pane writes to aim `fleet cmd` at its own terminal. Self-targeting is
/// the ordinary use of that verb — a worker compacting its own context — and it
/// exists for `cmd` only: the message self-send guard is untouched.
const SELF_TARGET: &str = "self";

#[derive(Parser)]
#[command(name = "fleet", version, about = "Talk to the other terminals in your FLEETOR fleet")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The verbs, in the order `fleetor_core::brief::VERBS` introduces them. The test
/// at the bottom pins the two lists together — a rename that only lands here
/// would leave both briefs teaching a command that no longer exists.
#[derive(Subcommand)]
enum Command {
    /// Write into one pane's terminal: `fleet send 2 "take the parser"`.
    Send {
        /// `orch`, or a worker as `2` / `w2` / `worker-2`.
        pane: String,
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },
    /// Write into every pane except your own. Use sparingly.
    Broadcast {
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },
    /// Answer whoever messaged you last.
    Reply {
        #[arg(trailing_var_arg = true, required = true)]
        text: Vec<String>,
    },
    /// Run `/clear` or `/compact` in a pane's terminal, with the reason you did.
    ///
    /// `--why` is `required` here rather than merely documented: it is a contract
    /// on the sender, enforced before anything enters the delivery path, the same
    /// class of check as `Hello.pane`. The log of *why* the fleet cleared or
    /// compacted is the reasoning chain the verb exists to keep, and a `--why`
    /// that could be skipped would be skipped.
    ///
    /// The command is `num_args(1..)` and **not** `trailing_var_arg`, unlike
    /// every message verb above: trailing args would swallow `--why` itself, and
    /// a model that wrote the flag last would silently send it as part of the
    /// command. This way both `fleet cmd 2 "/compact keep X" --why "…"` and
    /// `fleet cmd 2 /compact keep X --why "…"` mean the same thing.
    Cmd {
        /// `orch`, a worker as `2` / `w2` / `worker-2`, or `self` for your own.
        pane: String,
        /// `/clear`, or `/compact <what to keep>`. Anything else is refused.
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
        /// Why you decided to send it — a sentence, not the command restated.
        #[arg(long, required = true)]
        why: String,
    },
    /// Who exists and whether they are live.
    Roster,
    /// Your own pane name.
    Whoami,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            // `{e:#}` so the anyhow context chain ("connecting to …: No such file
            // or directory") is on one line the model can read and act on.
            eprintln!("fleet: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();

    // `whoami` answers from the environment alone: it is the first thing a
    // confused pane tries, and it must work even when the hub is down.
    if let Command::Whoami = cli.command {
        println!("{}", me()?);
        return Ok(ExitCode::SUCCESS);
    }

    let op = match cli.command {
        Command::Send { pane, text } => Op::Send {
            to: pane.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}"))?,
            text: join(text),
        },
        Command::Broadcast { text } => Op::Broadcast { text: join(text) },
        Command::Reply { text } => Op::Reply { text: join(text) },
        Command::Cmd { pane, command, why } => {
            Op::Cmd { to: target(&pane)?, command: join(command), why }
        }
        Command::Roster => Op::Roster,
        Command::Whoami => unreachable!("handled above"),
    };

    // One connection, one op, then exit — this process is spawned per Bash call.
    // A current-thread runtime is all that needs to exist for that.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    let result = runtime.block_on(async {
        let transport = UnixTransport::new(socket()?);
        let mut client = Client::connect(&transport, Hello::for_pane(me()?)).await?;
        client.call(op).await
    })?;

    Ok(if report(result) { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Turn the hub's answer into stdout/stderr, and say whether it succeeded. This
/// function is the contract the briefs describe, so it is the one worth reading
/// twice. It returns a bool rather than an `ExitCode` only so a test can assert
/// on it — `ExitCode` is deliberately opaque.
fn report(result: OpResult) -> bool {
    match result {
        OpResult::Delivered { msg_id, accepted: true, .. } => {
            // Deliberately not "delivered": the bytes reached a live terminal,
            // which is not a claim that the agent there has read them (L3).
            println!("accepted {msg_id}");
            true
        }
        OpResult::Delivered { accepted: false, detail, .. } => {
            eprintln!("fleet: not delivered — {}", detail.as_deref().unwrap_or("the pane refused"));
            false
        }
        OpResult::Roster { panes } => {
            for line in roster_lines(&panes) {
                println!("{line}");
            }
            true
        }
        OpResult::Error { message } => {
            eprintln!("fleet: {message}");
            false
        }
    }
}

/// One line per pane: its name, its state, and its context column (WP-04) —
/// `≈NN% (used/window tok)` when a gauge could be sampled, `—` otherwise. `—`
/// means *unknown*, never zero: a worker that has not completed a turn yet and
/// the orchestrator (never sampled — its transcript is the operator's own)
/// both render this way, and the brief tells orch not to read either as an
/// empty context. Pure and separate from `report` so the format is testable
/// without capturing stdout.
fn roster_lines(panes: &[PaneEntry]) -> Vec<String> {
    panes
        .iter()
        .map(|entry| {
            let state = format!("{:?}", entry.state).to_lowercase();
            let context = match entry.context {
                Some(c) => format!("≈{}% ({}/{} tok, from transcript)", c.pct, c.used_tokens, c.window_tokens),
                None => "—".to_string(),
            };
            format!("{:<10} {state:<10} {context}", entry.pane.to_string())
        })
        .collect()
}

/// Which pane is running this command. Refusing beats guessing: an unattributed
/// message would arrive from nobody and `fleet reply` would have nothing to
/// answer to.
fn me() -> Result<PaneId> {
    let raw = std::env::var(ENV_PANE).ok().filter(|s| !s.trim().is_empty()).with_context(|| {
        format!("{ENV_PANE} is not set — run this inside a FLEETOR pane, not a plain terminal")
    })?;
    raw.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{ENV_PANE}: {e}"))
}

/// The pane a `fleet cmd` is aimed at. `self` resolves here, in the one process
/// that already knows which pane it is, rather than becoming a `PaneId` spelling
/// — a `PaneId` that meant "whoever is asking" would be a different pane on
/// every side of the socket, and the hub would have to guess which.
fn target(pane: &str) -> Result<PaneId> {
    if pane.trim().eq_ignore_ascii_case(SELF_TARGET) {
        return me();
    }
    pane.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}"))
}

fn socket() -> Result<PathBuf> {
    let raw = std::env::var(ENV_SOCKET).ok().filter(|s| !s.trim().is_empty()).with_context(|| {
        format!("{ENV_SOCKET} is not set — the fleet shell sets it on every pane it spawns")
    })?;
    Ok(PathBuf::from(raw))
}

/// `fleet send 2 hello there` and `fleet send 2 "hello there"` mean the same
/// thing. Models quote inconsistently and a dropped word is a silent corruption.
fn join(text: Vec<String>) -> String {
    text.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The briefs teach exactly these verbs. If a subcommand is renamed here and
    /// nowhere else, every pane in the fleet is taught a command that exits 2.
    #[test]
    fn the_subcommands_are_exactly_the_verbs_the_briefs_teach() {
        let names: Vec<String> =
            Cli::command().get_subcommands().map(|c| c.get_name().to_string()).collect();
        assert_eq!(names, fleetor_core::brief::VERBS.to_vec());
    }

    /// A model writing `fleet send 2 take the parser` must not lose "the parser".
    #[test]
    fn unquoted_text_is_rejoined_rather_than_truncated() {
        let cli = Cli::try_parse_from(["fleet", "send", "2", "take", "the", "parser"]).unwrap();
        let Command::Send { pane, text } = cli.command else { panic!("expected send") };
        assert_eq!(pane, "2");
        assert_eq!(join(text), "take the parser");
    }

    /// Every spelling `PaneId` accepts must survive the CLI boundary, because the
    /// brief shows both `fleet send 2` and `fleet send orch`.
    #[test]
    fn the_pane_argument_accepts_what_a_model_actually_types() {
        for (arg, expected) in
            [("2", PaneId::Worker(2)), ("worker-2", PaneId::Worker(2)), ("orch", PaneId::Orch)]
        {
            let cli = Cli::try_parse_from(["fleet", "send", arg, "hi"]).unwrap();
            let Command::Send { pane, .. } = cli.command else { panic!("expected send") };
            assert_eq!(pane.parse::<PaneId>().unwrap(), expected, "{arg}");
        }
    }

    /// An empty message would be typed into a live terminal as a bare newline —
    /// a submitted empty turn. Clap must refuse it before the hub ever sees it.
    #[test]
    fn a_message_with_no_text_is_refused() {
        assert!(Cli::try_parse_from(["fleet", "send", "2"]).is_err());
        assert!(Cli::try_parse_from(["fleet", "broadcast"]).is_err());
        assert!(Cli::try_parse_from(["fleet", "reply"]).is_err());
    }

    // --- the command channel (D-045) ------------------------------------------

    /// `--why` is a contract on the sender, enforced by clap before anything
    /// reaches the hub — the same class of check as a `Hello` naming its pane.
    /// A command whose reason was optional would arrive without one.
    #[test]
    fn a_command_without_a_why_is_refused_by_the_parser() {
        assert!(Cli::try_parse_from(["fleet", "cmd", "2", "/clear"]).is_err());
        assert!(Cli::try_parse_from(["fleet", "cmd", "2", "--why", "stale"]).is_err(), "no command");
        assert!(Cli::try_parse_from(["fleet", "cmd", "--why", "stale"]).is_err(), "no pane");
        assert!(Cli::try_parse_from(["fleet", "cmd", "2", "/clear", "--why", "stale"]).is_ok());
    }

    /// The reason `command` is `num_args(1..)` rather than `trailing_var_arg`
    /// like every message verb: trailing args swallow the flag, and a model that
    /// wrote `--why` last would have sent it as part of the command. Both
    /// spellings a model actually types must mean the same thing.
    #[test]
    fn the_why_survives_however_the_model_quotes_the_command() {
        for argv in [
            vec!["fleet", "cmd", "2", "/compact keep the parser", "--why", "task block done"],
            vec!["fleet", "cmd", "2", "/compact", "keep", "the", "parser", "--why", "task block done"],
            vec!["fleet", "cmd", "--why", "task block done", "2", "/compact", "keep", "the", "parser"],
        ] {
            let cli = Cli::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            let Command::Cmd { pane, command, why } = cli.command else { panic!("expected cmd") };
            assert_eq!(pane, "2");
            assert_eq!(join(command), "/compact keep the parser", "{argv:?}");
            assert_eq!(why, "task block done", "{argv:?}");
        }
    }

    /// `self` is resolved here, where the process already knows which pane it is.
    /// Every other spelling still goes through `PaneId`, which is untouched.
    #[test]
    fn the_self_target_resolves_to_the_pane_running_the_command() {
        std::env::set_var(ENV_PANE, "worker-3");
        for spelling in ["self", "SELF", " self "] {
            assert_eq!(target(spelling).unwrap(), PaneId::Worker(3), "{spelling}");
        }
        assert_eq!(target("orch").unwrap(), PaneId::Orch);
        assert_eq!(target("2").unwrap(), PaneId::Worker(2));
        assert!(target("sidebar").is_err(), "a non-pane is still a non-pane");
        std::env::remove_var(ENV_PANE);
    }

    /// The two exit codes the briefs promise. `accepted` is the *only* zero.
    #[test]
    fn only_an_accepted_delivery_exits_zero() {
        assert!(report(OpResult::Delivered { msg_id: "msg-1".into(), accepted: true, detail: None }));

        let refused = OpResult::Delivered {
            msg_id: "msg-2".into(),
            accepted: false,
            detail: Some("worker-3 is dead".into()),
        };
        assert!(!report(refused), "a refused send must not exit zero");
        assert!(!report(OpResult::Error { message: "no such pane".into() }));
    }

    // --- the roster's context column (WP-04) ------------------------------------

    use fleetor_core::pane::{ContextGauge, PaneState};

    /// An unsampled pane — orch always, a worker before its first turn —
    /// renders `—`, never `0%`: a blank column must read as *unknown*, not
    /// "definitely empty."
    #[test]
    fn an_unsampled_pane_renders_an_em_dash_not_a_zero() {
        let lines = roster_lines(&[PaneEntry::new(PaneId::Orch, PaneState::Live)]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains('—'), "{}", lines[0]);
        assert!(!lines[0].contains('%'), "no percent may appear for an unsampled pane: {}", lines[0]);
    }

    /// A sampled worker's line carries its pane name, state, and the honest
    /// `≈` figure — never a bare number that could be mistaken for a promise.
    #[test]
    fn a_sampled_worker_renders_its_gauge_with_the_approx_label() {
        let entry = PaneEntry::new(PaneId::Worker(2), PaneState::Live)
            .with_context(ContextGauge::new(64_000, 128_000));
        let lines = roster_lines(&[entry]);
        assert!(lines[0].contains("worker-2"), "{}", lines[0]);
        assert!(lines[0].contains("live"), "{}", lines[0]);
        assert!(lines[0].contains('≈'), "every figure carries its honesty label: {}", lines[0]);
        assert!(lines[0].contains("50%"), "{}", lines[0]);
    }

    /// One line per pane, in the order the roster arrived — the CLI must not
    /// silently reorder or drop a pane the caller has to account for.
    #[test]
    fn roster_lines_covers_every_pane_in_order() {
        let panes = vec![
            PaneEntry::new(PaneId::Orch, PaneState::Live),
            PaneEntry::new(PaneId::Worker(1), PaneState::Dead),
            PaneEntry::new(PaneId::Worker(2), PaneState::Spawning),
        ];
        let lines = roster_lines(&panes);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("orch"));
        assert!(lines[1].starts_with("worker-1") && lines[1].contains("dead"));
        assert!(lines[2].starts_with("worker-2") && lines[2].contains("spawning"));
    }
}
