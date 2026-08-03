//! `fleet` — the whole agent-facing surface of a FLEETOR fleet (D-030).
//!
//! Five verbs, one unix socket, no MCP server. A pane's `claude` runs this
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
use fleetor_core::pane::PaneId;
use fleetor_core::wire::{Hello, Op, OpResult};
use fleetor_ipc::{Client, UnixTransport};
use std::path::PathBuf;
use std::process::ExitCode;

/// Set by the spawn path on every pane. Without it the hub cannot attribute a
/// message, so every verb but `whoami` refuses rather than guessing.
const ENV_PANE: &str = "FLEETOR_PANE";
/// Set by the spawn path; the hub's unix socket.
const ENV_SOCKET: &str = "FLEET_SOCKET";

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
        Command::Send { pane, text } => Op::PaneSend {
            to: pane.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}"))?,
            text: join(text),
        },
        Command::Broadcast { text } => Op::PaneBroadcast { text: join(text) },
        Command::Reply { text } => Op::PaneReply { text: join(text) },
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
            for entry in panes {
                let state = format!("{:?}", entry.state).to_lowercase();
                println!("{:<10} {state}", entry.pane.to_string());
            }
            true
        }
        OpResult::Error { message } => {
            eprintln!("fleet: {message}");
            false
        }
        // Every other variant belongs to the pre-D-030 headless surface Phase 5
        // deletes. Reaching one means the hub answered a different question than
        // we asked, which is a bug worth naming rather than exiting zero on.
        other => {
            eprintln!("fleet: unexpected answer from the hub: {other:?}");
            false
        }
    }
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
        // A hub answering a different question is a failure, not a quiet success.
        assert!(!report(OpResult::Ack));
    }
}
