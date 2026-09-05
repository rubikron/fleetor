//! `fleet` — the whole agent-facing surface of a FLEETOR fleet (D-030).
//!
//! Nine verbs, one unix socket, no MCP server. A pane's `claude` runs this
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
use fleetor_core::task::{TaskEntry, TaskStatus};
use fleetor_core::wire::{Hello, Op, OpResult, TaskAction};
use fleetor_ipc::{Client, UnixTransport};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;

mod done;

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
    ///
    /// `fleet send operator "…"` addresses the human instead. They have no
    /// terminal, so that one answers `recorded` rather than `accepted` — it is
    /// in the log and in their inbox, which is the whole of what can be
    /// promised about a person.
    Send {
        /// `orch`, `operator`, or a worker as `2` / `w2` / `worker-2`.
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
    /// The task board: post a block, update one, or read the board (WP-05).
    ///
    /// One verb with three subcommands rather than three verbs, because the
    /// requirements spend the verb budget once: `task` is one word both briefs
    /// have to teach and one entry in `brief::VERBS`.
    Task {
        #[command(subcommand)]
        action: TaskCmd,
    },
    /// Close a block: run its check here, then send `orch` the receipt (WP-06).
    ///
    /// The only verb that runs something. It runs it **in this process**, in the
    /// pane's own worktree — the hub never executes anything and never sees the
    /// command. What crosses the socket is an ordinary [`Op::Send`] to `orch`.
    ///
    /// **This verb's exit code still means delivery.** The check's own exit code
    /// travels in the message body, which is where a reader can act on it. See
    /// `done.rs` for why conflating the two would break the one promise both
    /// briefs make verbatim.
    Done {
        /// The block this is a receipt for — `fleet task list` shows the ids.
        #[arg(value_name = "TASK-ID")]
        task: String,
        /// The check to run here: whatever the block's technical criteria say.
        /// `trailing_var_arg` for the reason the message verbs use it — a model
        /// that forgets the quotes must not lose half its command.
        #[arg(trailing_var_arg = true, required = true, value_name = "CHECK")]
        check: Vec<String>,
    },
    /// Say the whole goal is met, and hand the work back to the operator (WP-13).
    ///
    /// **`orch`'s verb, and a different altitude from `done`.** A receipt closes
    /// one block; this closes the mission. It reaches the log and nothing else —
    /// no terminal is written to, so it answers `recorded`.
    ///
    /// The fields are flat and repeatable for the reason `task post`'s are: a
    /// weak model emits `--evidence "…" --evidence "…"` far more reliably than a
    /// nested document, and `--evidence` is required at least once because a
    /// claim with nothing checkable behind it cannot be argued with.
    Handoff {
        /// What the fleet built, in the operator's own terms.
        #[arg(long, required = true)]
        built: String,
        /// How anyone could check it — a command, a branch, a file. Repeatable.
        #[arg(long, required = true, action = clap::ArgAction::Append, value_name = "CHECK")]
        evidence: Vec<String>,
        /// What is unfinished or uncertain. Repeatable, and optional: a mission
        /// with no loose ends must not have to invent one.
        #[arg(long, action = clap::ArgAction::Append, value_name = "LOOSE-END")]
        open: Vec<String>,
    },
    /// Who exists and whether they are live.
    Roster,
    /// Your own pane name.
    Whoami,
}

/// The three things `fleet task` does.
///
/// **Flat flags, never JSON.** A weak model emits `--outcome "…" --crit-t "…"`
/// far more reliably than a nested document, and a malformed JSON block would
/// cost a whole turn to diagnose from a parser error.
#[derive(Subcommand)]
enum TaskCmd {
    /// Put a block on the board: what it enables, how to check it, and whose it is.
    ///
    /// Posting assigns nobody — the board is the record. Send the worker its job
    /// with `fleet send` afterwards; that is the assignment.
    Post {
        /// The pane whose job this is: `orch`, or a worker as `2` / `worker-2`.
        #[arg(long, value_name = "PANE")]
        to: String,
        /// What this execution enables when it is done.
        #[arg(long, required = true)]
        outcome: String,
        /// A checkable technical criterion, command-shaped where possible.
        /// Repeat for more than one.
        #[arg(long = "crit-t", required = true, action = clap::ArgAction::Append, value_name = "CHECK")]
        crit_t: Vec<String>,
        /// Which part of the confirmed vision this block serves. Repeatable.
        #[arg(long = "crit-s", required = true, action = clap::ArgAction::Append, value_name = "VISION")]
        crit_s: Vec<String>,
        /// Detail the worker needs that does not fit in the outcome.
        #[arg(long)]
        instructions: Option<String>,
        /// The block this one was cut out of, by id.
        #[arg(long, value_name = "TASK-ID")]
        parent: Option<String>,
        /// The block this stream of work comes back together in, by id.
        #[arg(long = "converges-on", value_name = "TASK-ID")]
        converges_on: Option<String>,
    },
    /// Append a claim to a block: a status, a note, or both.
    ///
    /// Anyone may update any block — the log records who, and that is the whole
    /// of the accountability. Nothing is enforced: any status may follow any
    /// other, because a worker who finds a failing criterion after saying `done`
    /// has to be able to say so.
    Update {
        /// The block, by id — `fleet task list` shows them.
        #[arg(value_name = "TASK-ID")]
        task: String,
        /// One of `planned`, `claimed`, `done`, `dropped`.
        #[arg(long)]
        status: Option<String>,
        /// What changed, and what you checked. Required if `--status` is absent.
        #[arg(long)]
        note: Option<String>,
    },
    /// Read the board back: one line per block, newest streams last.
    List {
        /// Show each block's criteria, instructions and update trail too.
        #[arg(long)]
        full: bool,
    },
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
    // `task list --full` is a rendering choice, not something the hub is told:
    // the wire carries the board, this side decides how much of it to print.
    let full = matches!(cli.command, Command::Task { action: TaskCmd::List { full: true } });

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
        Command::Reply { text } => Op::Reply { text: reply_text(text)? },
        Command::Cmd { pane, command, why } => {
            Op::Cmd { to: target(&pane)?, command: join(command), why }
        }
        Command::Task { action } => Op::Task { action: task_action(action)? },
        // The check runs *before* the connection is opened — a receipt describes
        // something that already happened, and a socket held open for the length
        // of a test suite is a connection doing nothing but waiting.
        Command::Done { task, check } => done::op(me()?, &task, &join(check))?,
        Command::Handoff { built, evidence, open } => handoff_op(me()?, built, evidence, open)?,
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

    Ok(if report(result, full) { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// The `fleet task` wire payload. Parsing happens here rather than at the hub
/// for the reason `PaneId` does: a typo costs a local error the model reads on
/// its own stderr, not a socket round trip. What the hub still checks is what
/// only it can — that the block being updated is actually on the board.
fn task_action(action: TaskCmd) -> Result<TaskAction> {
    Ok(match action {
        TaskCmd::Post { to, outcome, crit_t, crit_s, instructions, parent, converges_on } => {
            TaskAction::Post {
                worker: to.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}"))?,
                outcome,
                technical: crit_t,
                semantic: crit_s,
                instructions,
                parent,
                converges_on,
            }
        }
        TaskCmd::Update { task, status, note } => TaskAction::Update {
            task,
            status: status
                .map(|s| TaskStatus::parse(&s).map_err(|e| anyhow::anyhow!("{e}")))
                .transpose()?,
            note,
        },
        TaskCmd::List { .. } => TaskAction::List,
    })
}

/// The `fleet handoff` wire payload (WP-13).
///
/// **The one check here is who is asking.** A handoff is the orchestrator's
/// declaration that the whole goal is met; a worker has one block, and the verb
/// that closes one is `fleet done`. Refused locally rather than at the hub, for
/// the reason `done.rs` refuses `orch` locally: the sender reads its own stderr
/// and the refusal can say what to type instead. Everything about the *content*
/// is checked once, in `fleetor_core::handoff`, where the hub can see it too.
fn handoff_op(me: PaneId, built: String, evidence: Vec<String>, open: Vec<String>) -> Result<Op> {
    if me != PaneId::Orch {
        anyhow::bail!(
            "`fleet handoff` is how `orch` tells the operator the whole goal is met, and you \
             are {me}. To close your own block, run `fleet done <task-id> \"<check>\"`"
        );
    }
    Ok(Op::Handoff { built, evidence, open })
}

/// Turn the hub's answer into stdout/stderr, and say whether it succeeded. This
/// function is the contract the briefs describe, so it is the one worth reading
/// twice. It returns a bool rather than an `ExitCode` only so a test can assert
/// on it — `ExitCode` is deliberately opaque.
fn report(result: OpResult, full: bool) -> bool {
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
        // "recorded", not "assigned" and not "accepted": what happened is that
        // something reached the log and no terminal was written to. For a task
        // claim that means nobody was told to do anything — that is the `fleet
        // send` the brief tells the orchestrator to write next. For a message
        // to the operator it means the human has it in their inbox whenever
        // they look, which is the most any of this could honestly promise
        // about a person.
        OpResult::Recorded { record_id } => {
            println!("recorded {record_id}");
            true
        }
        OpResult::Board { tasks } => {
            for line in board_lines(&tasks, full) {
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

/// The board as a compact tree: one line per block, indented under its parent,
/// `--full` adding its criteria, instructions and update trail beneath it.
///
/// Compact by default because the reading model is a pane with finite context —
/// the whole board has to be affordable to look at, or it stops being looked at.
/// Pure and separate from `report` so the shape is testable without capturing
/// stdout, exactly like `roster_lines`.
fn board_lines(tasks: &[TaskEntry], full: bool) -> Vec<String> {
    if tasks.is_empty() {
        return vec![
            "the board is empty — `fleet task post --to <pane> --outcome \"…\" \
             --crit-t \"…\" --crit-s \"…\"` puts the first block on it"
                .to_string(),
        ];
    }
    let (order, cyclic) = tree_order(tasks);
    let mut lines = Vec::new();
    if cyclic {
        // Tolerated, not rejected: a cycle in the links is somebody's note about
        // how the work fits together, and validating it into a legal graph is the
        // workflow engine this board is deliberately not.
        lines.push(
            "note: the parent links contain a cycle, so the board is listed flat. \
             Nothing depends on the shape — fix it or leave it"
                .to_string(),
        );
    }
    for (index, depth) in order {
        let entry = &tasks[index];
        let indent = "  ".repeat(depth);
        lines.push(format!("{indent}{}", summary(entry)));
        if full {
            lines.extend(details(entry).into_iter().map(|line| format!("{indent}    {line}")));
        }
    }
    lines
}

/// `task-… [claimed] worker-2 — the parser accepts nested groups`. The id comes
/// first because it is the thing that gets typed back into `fleet task update`.
fn summary(entry: &TaskEntry) -> String {
    let converges = entry
        .block
        .converges_on
        .as_deref()
        .map(|id| format!("  → converges on {id}"))
        .unwrap_or_default();
    format!(
        "{} [{}] {} — {}{converges}",
        entry.id, entry.status, entry.block.worker, entry.block.outcome
    )
}

/// What `--full` adds. Every claim is attributed, because who said a block was
/// done is the part WP-06's review will need.
fn details(entry: &TaskEntry) -> Vec<String> {
    let mut lines: Vec<String> =
        entry.block.technical.iter().map(|c| format!("technical: {c}")).collect();
    lines.extend(entry.block.semantic.iter().map(|c| format!("vision: {c}")));
    if let Some(instructions) = &entry.block.instructions {
        lines.push(format!("instructions: {instructions}"));
    }
    lines.push(format!("posted by {}", entry.posted_by));
    lines.extend(entry.updates.iter().map(|update| {
        let what = match (update.status, update.note.as_deref()) {
            (Some(status), Some(note)) => format!("[{status}] {note}"),
            (Some(status), None) => format!("[{status}]"),
            (None, Some(note)) => note.to_string(),
            // `TaskUpdate::new` refuses this; rendered rather than panicked on,
            // because a log written by an older build must still print.
            (None, None) => "—".to_string(),
        };
        format!("{} — {what}", update.from)
    }));
    lines
}

/// Depth-first over the parent links, returning `(index, depth)` in render order.
/// A block whose parent id names nothing on the board is a root — a dangling link
/// is data, not an error, and the board still has to print.
fn tree_order(tasks: &[TaskEntry]) -> (Vec<(usize, usize)>, bool) {
    let index: HashMap<&str, usize> =
        tasks.iter().enumerate().map(|(i, task)| (task.id.as_str(), i)).collect();
    let parents: Vec<Option<usize>> = tasks
        .iter()
        .map(|task| task.block.parent.as_deref().and_then(|id| index.get(id).copied()))
        .collect();

    if has_cycle(&parents) {
        return ((0..tasks.len()).map(|i| (i, 0)).collect(), true);
    }

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (child, parent) in parents.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(child),
            None => roots.push(child),
        }
    }

    let mut out = Vec::new();
    for root in roots {
        walk(root, 0, &children, &mut out);
    }
    (out, false)
}

fn walk(node: usize, depth: usize, children: &[Vec<usize>], out: &mut Vec<(usize, usize)>) {
    out.push((node, depth));
    for child in &children[node] {
        walk(*child, depth + 1, children, out);
    }
}

fn has_cycle(parents: &[Option<usize>]) -> bool {
    (0..parents.len()).any(|start| {
        let mut seen = HashSet::new();
        let mut node = Some(start);
        while let Some(current) = node {
            if !seen.insert(current) {
                return true;
            }
            node = parents[current];
        }
        false
    })
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

/// The body of a `fleet reply`, or a refusal if the pane wrote a recipient
/// where the body starts (D-077).
///
/// **This is a measured defect, not a hypothetical one.** `reply` is
/// `trailing_var_arg` like every message verb, and it has no recipient field —
/// so `fleet reply worker-3 "Agreed"` parses cleanly, prepends `worker-3` to the
/// *message* and delivers the whole thing to whoever messaged the sender last.
/// A 5m 40s run produced nine such misdeliveries: the brief teaches
/// `fleet send orch "<text>"` on the line above `fleet reply "<text>"`, and
/// panes pattern-matched the argument across. Nothing failed, so nothing was
/// corrected — two panes spent turns diagnosing "tangled cross-replies" and one
/// re-sent three confirmations it had already sent.
///
/// **The discriminator is the first argv element, not the first word.** A pane
/// must still be able to start a sentence with a peer's name, and
/// `fleet reply "worker-3 said the annex is out"` arrives here as a single
/// element that is not a pane name. Only `fleet reply worker-3 "…"` — a
/// separate argument that [`PaneId`] itself accepts — is refused. Asking
/// `PaneId::from_str` rather than matching spellings by hand is what keeps this
/// exactly as wide as `fleet send`'s own recipient: `orch`, `lead`, `o`,
/// `operator`, `2`, `w2`, `worker2`, `worker-2` and the rest all move together.
///
/// **Tier 1.4 holds because this refuses at parse.** It runs where
/// `Command::Send`'s `pane.parse::<PaneId>()` runs — before the runtime is
/// built, before the socket is dialled, before any `Op` exists — so it is the
/// same shape of failure as `fleet send worker-9`, and nothing between a
/// `fleet send` and a pty gained the ability to refuse anything.
fn reply_text(text: Vec<String>) -> Result<String> {
    if let Some(first) = text.first() {
        if first.parse::<PaneId>().is_ok() {
            anyhow::bail!(
                "`fleet reply` does not take a recipient — use \
                 `fleet send {first} \"<text>\"`, or drop the name to answer \
                 whoever messaged you last"
            );
        }
    }
    Ok(join(text))
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

    // --- `fleet reply` takes no recipient (D-077) ------------------------------

    /// argv in, and out comes exactly what `run()` would put on the wire for a
    /// `fleet reply` — or the refusal it would print instead. Goes through clap
    /// on purpose: the whole discriminator is *how argv was split*, and a test
    /// that built the `Vec<String>` by hand would be testing its own assumption.
    fn reply_from(argv: &[&str]) -> Result<String> {
        let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        let Command::Reply { text } = cli.command else { panic!("expected reply") };
        reply_text(text)
    }

    /// The measured defect: nine misdeliveries in one 5m 40s run, because
    /// `trailing_var_arg` makes `fleet reply worker-3 "…"` *succeed* with the
    /// name glued to the front of the body and the body sent to somebody else.
    /// Every spelling `fleet send` takes as a recipient has to be caught, or the
    /// pane that types the uncaught one is back to a silent misroute.
    #[test]
    fn a_reply_that_names_a_recipient_is_refused_for_every_spelling_send_accepts() {
        for name in [
            "orch",
            "orchestrator",
            "lead",
            "o",
            "operator",
            "critic",
            "0",
            "3",
            "255",
            "w3",
            "worker3",
            "worker-3",
            "OrCh",
            "Worker-3",
            " orch ",
        ] {
            assert!(
                name.parse::<PaneId>().is_ok(),
                "{name:?}: this test is only meaningful for names `fleet send` accepts"
            );
            let refused = reply_from(&["fleet", "reply", name, "Agreed", "—", "Rooftop", "Garden"]);
            assert!(refused.is_err(), "{name:?} was accepted as the start of a message body");
        }
    }

    /// The refusal is read by a model that then types the next command, so it
    /// names the verb that *does* take a recipient and hands back the name it
    /// was given. Pinned exactly: a vaguer sentence is the same silent misroute
    /// one turn later.
    #[test]
    fn the_refusal_names_the_verb_that_does_take_a_recipient() {
        let refused = reply_from(&["fleet", "reply", "worker-3", "Agreed — Rooftop Garden"]);
        assert_eq!(
            refused.unwrap_err().to_string(),
            "`fleet reply` does not take a recipient — use `fleet send worker-3 \"<text>\"`, \
             or drop the name to answer whoever messaged you last"
        );
    }

    /// **The half that matters more.** A pane must be able to open a message
    /// with a peer's name — quoting one, reporting on one, arguing with one.
    /// Here the name is *inside* the single argv element the shell handed over,
    /// so it is prose, and prose goes through untouched.
    #[test]
    fn a_reply_may_begin_with_a_peers_name_when_the_name_is_prose() {
        for (argv, expected) in [
            (
                vec!["fleet", "reply", "worker-3 said the annex is out"],
                "worker-3 said the annex is out",
            ),
            (vec!["fleet", "reply", "orch wants the parser first"], "orch wants the parser first"),
            (vec!["fleet", "reply", "2 of the three checks pass"], "2 of the three checks pass"),
            // Unquoted, and the name is not in front: still a message.
            (vec!["fleet", "reply", "ask", "worker-3"], "ask worker-3"),
        ] {
            assert_eq!(reply_from(&argv).unwrap(), expected, "{argv:?}");
        }
    }

    /// The check asks `PaneId` rather than matching spellings of its own, so it
    /// is exactly as wide as `fleet send`'s recipient and cannot drift from it.
    /// Note `self`: `fleet send self` is not a thing (only `fleet cmd` resolves
    /// it), so a reply may start with the word. A first token starting with `-`
    /// never reaches here at all — clap rejects it ahead of us, `-1` included.
    ///
    /// One name `PaneId` accepts is missing from both lists in this file, and
    /// not because it is uncovered: WP-15's Tier 1.4 grep forbids the string in
    /// every file on the delivery path, and this is one of them. It is exercised
    /// in `tests/reply_recipient.rs`, which is not.
    #[test]
    fn the_refusal_is_exactly_as_wide_as_the_recipient_fleet_send_accepts() {
        for candidate in [
            "orch", "lead", "o", "operator", "critic", "2", "w2", "worker2", "worker-2", "self",
            "w", "worker", "worker-", "worker_3", "256", "3pm", "Agreed", "annex", "orch,",
            "worker-3 said the annex is out",
        ] {
            let is_a_pane_name = candidate.parse::<PaneId>().is_ok();
            let refused = reply_from(&["fleet", "reply", candidate, "text"]).is_err();
            assert_eq!(refused, is_a_pane_name, "{candidate:?}");
        }
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
        assert!(report(
            OpResult::Delivered { msg_id: "msg-1".into(), accepted: true, detail: None },
            false
        ));

        let refused = OpResult::Delivered {
            msg_id: "msg-2".into(),
            accepted: false,
            detail: Some("worker-3 is dead".into()),
        };
        assert!(!report(refused, false), "a refused send must not exit zero");
        assert!(!report(OpResult::Error { message: "no such pane".into() }, false));
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

    // --- the task board (WP-05) -------------------------------------------------

    use fleetor_core::task::{TaskBlock, TaskEntry, TaskNote};

    fn entry(id: &str, parent: Option<&str>, worker: u8) -> TaskEntry {
        TaskEntry {
            id: id.into(),
            block: TaskBlock::new(
                &format!("outcome of {id}"),
                &["cargo test -p parser".to_string()],
                &["one grammar".to_string()],
                PaneId::Worker(worker),
                Some("start from the tokenizer"),
                parent,
                None,
            )
            .unwrap(),
            posted_by: PaneId::Orch,
            posted_at: 1,
            status: TaskStatus::Planned,
            updates: Vec::new(),
        }
    }

    /// Everything a block needs to be worth posting is `required` in clap, not
    /// documented and hoped for — the same contract-on-the-sender argument that
    /// made `fleet cmd --why` required (D-045). A block with an outcome and no
    /// checkable criterion cannot be argued with, which is the whole point of one.
    #[test]
    fn a_block_without_criteria_or_an_owner_is_refused_by_the_parser() {
        let complete = ["fleet", "task", "post", "--to", "2", "--outcome", "o", "--crit-t", "t", "--crit-s", "s"];
        assert!(Cli::try_parse_from(complete).is_ok());

        for dropped in ["--to", "--outcome", "--crit-t", "--crit-s"] {
            let mut argv: Vec<&str> = Vec::new();
            let mut skip = false;
            for arg in complete {
                if skip {
                    skip = false;
                    continue;
                }
                if arg == dropped {
                    skip = true;
                    continue;
                }
                argv.push(arg);
            }
            assert!(Cli::try_parse_from(&argv).is_err(), "{dropped} must be required: {argv:?}");
        }
    }

    /// A slice of work usually has more than one way to check it, so both
    /// criteria flags repeat rather than taking one string a model would have to
    /// join by hand.
    #[test]
    fn the_criteria_flags_repeat_and_keep_their_order() {
        let cli = Cli::try_parse_from([
            "fleet", "task", "post", "--to", "worker-3", "--outcome", "the parser lands",
            "--crit-t", "cargo test -p parser", "--crit-t", "fleet task list shows it",
            "--crit-s", "one grammar", "--parent", "task-1-0", "--converges-on", "task-1-9",
        ])
        .expect("a complete block");
        let Command::Task { action } = cli.command else { panic!("expected task") };
        let TaskAction::Post { worker, technical, semantic, parent, converges_on, .. } =
            task_action(action).unwrap()
        else {
            panic!("expected post")
        };
        assert_eq!(worker, PaneId::Worker(3));
        assert_eq!(technical, vec!["cargo test -p parser", "fleet task list shows it"]);
        assert_eq!(semantic, vec!["one grammar"]);
        assert_eq!(parent.as_deref(), Some("task-1-0"));
        assert_eq!(converges_on.as_deref(), Some("task-1-9"));
    }

    /// A status is parsed here, where a typo costs a local error rather than a
    /// socket round trip — and the refusal names the four words the board uses.
    #[test]
    fn a_status_is_parsed_before_the_wire_and_a_typo_names_the_four() {
        let update = |argv: &[&str]| {
            let cli = Cli::try_parse_from(argv).expect("parses");
            let Command::Task { action } = cli.command else { panic!("expected task") };
            task_action(action)
        };
        let TaskAction::Update { task, status, note } =
            update(&["fleet", "task", "update", "task-1-0", "--status", "done"]).unwrap()
        else {
            panic!("expected update")
        };
        assert_eq!(task, "task-1-0");
        assert_eq!(status, Some(TaskStatus::Done));
        assert_eq!(note, None);

        let why = update(&["fleet", "task", "update", "task-1-0", "--status", "in-progress"])
            .expect_err("must be refused")
            .to_string();
        for status in fleetor_core::task::TASK_STATUSES {
            assert!(why.contains(status), "the refusal names {status}: {why}");
        }

        // A note alone is a legitimate update: something on the record without a
        // claim of progress that has not happened.
        assert!(update(&["fleet", "task", "update", "task-1-0", "--note", "blocked"]).is_ok());
    }

    /// The empty board says what to type next. A blank answer would read as a
    /// broken command rather than an empty record.
    #[test]
    fn an_empty_board_says_how_to_put_something_on_it() {
        let lines = board_lines(&[], false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("fleet task post"), "{}", lines[0]);
    }

    /// The compact form is one line per block: the id first (it is what gets
    /// typed back), then the claimed status, the owner, and the outcome.
    #[test]
    fn the_compact_board_is_one_line_per_block_with_the_id_first() {
        let mut done = entry("task-1-1", None, 3);
        done.status = TaskStatus::Done;
        let lines = board_lines(&[entry("task-1-0", None, 2), done], false);
        assert_eq!(lines.len(), 2, "one line each: {lines:#?}");
        assert!(lines[0].starts_with("task-1-0 "), "{}", lines[0]);
        assert!(lines[0].contains("[planned]") && lines[0].contains("worker-2"), "{}", lines[0]);
        assert!(lines[1].contains("[done]") && lines[1].contains("worker-3"), "{}", lines[1]);
        assert!(!lines[0].contains("cargo test"), "criteria are on demand: {}", lines[0]);
    }

    /// Children indent under their parent, and a `converges-on` link is a suffix
    /// rather than a second tree — a stream of work has one place it was cut from
    /// and may come back together anywhere.
    #[test]
    fn children_indent_under_their_parent_and_convergence_is_a_suffix() {
        let mut child = entry("task-1-1", Some("task-1-0"), 3);
        child.block.converges_on = Some("task-1-9".into());
        let lines = board_lines(&[entry("task-1-0", None, 2), child], false);
        assert!(!lines[0].starts_with(' '), "a root is flush left: {}", lines[0]);
        assert!(lines[1].starts_with("  task-1-1"), "a child indents: {}", lines[1]);
        assert!(lines[1].contains("→ converges on task-1-9"), "{}", lines[1]);
    }

    /// A parent id naming nothing on the board is data, not an error — the block
    /// renders as a root and the board still prints.
    #[test]
    fn a_dangling_parent_link_renders_as_a_root() {
        let lines = board_lines(&[entry("task-1-1", Some("task-nowhere"), 2)], false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("task-1-1"), "{}", lines[0]);
    }

    /// Cycles are tolerated: rendered flat with a note, never validated into a
    /// legal graph. A board that refused a cycle would be a workflow engine.
    #[test]
    fn a_cycle_renders_flat_with_a_note_rather_than_failing() {
        let lines = board_lines(
            &[entry("task-1-0", Some("task-1-1"), 2), entry("task-1-1", Some("task-1-0"), 3)],
            false,
        );
        assert!(lines[0].starts_with("note:") && lines[0].contains("cycle"), "{}", lines[0]);
        assert_eq!(lines.len(), 3, "the note plus both blocks: {lines:#?}");
        assert!(lines[1].starts_with("task-1-0") && lines[2].starts_with("task-1-1"), "{lines:#?}");
    }

    /// `--full` is the criteria-on-demand switch: the definition of done, the
    /// vision link, and every attributed claim made since.
    #[test]
    fn full_shows_the_criteria_and_the_attributed_update_trail() {
        let mut task = entry("task-1-0", None, 2);
        task.updates = vec![
            TaskNote { from: PaneId::Worker(2), at: 2, status: Some(TaskStatus::Claimed), note: Some("on it".into()) },
            TaskNote { from: PaneId::Worker(4), at: 3, status: None, note: Some("crit 2 fails".into()) },
        ];
        let full = board_lines(&[task], true).join("\n");
        assert!(full.contains("technical: cargo test -p parser"), "{full}");
        assert!(full.contains("vision: one grammar"), "{full}");
        assert!(full.contains("instructions: start from the tokenizer"), "{full}");
        assert!(full.contains("posted by orch"), "{full}");
        assert!(full.contains("worker-2 — [claimed] on it"), "the trail is attributed: {full}");
        assert!(full.contains("worker-4 — crit 2 fails"), "a peer may claim too: {full}");
    }

    /// A board answer is a successful op — it is a record being read, not a
    /// delivery, so nothing about a pty is claimed either way.
    #[test]
    fn a_recorded_claim_and_a_board_read_both_exit_zero() {
        assert!(report(OpResult::Recorded { record_id: "task-1-0".into() }, false));
        assert!(report(OpResult::Board { tasks: vec![entry("task-1-0", None, 2)] }, true));
        assert!(report(OpResult::Board { tasks: vec![] }, false), "an empty board is not a failure");
    }

    // --- the receipt (WP-06) ----------------------------------------------------

    /// A receipt is about a block and reports a command. Neither half is optional:
    /// a receipt with no check ran nothing, and one with no block is evidence
    /// about nothing.
    #[test]
    fn a_receipt_without_a_block_or_a_check_is_refused_by_the_parser() {
        assert!(Cli::try_parse_from(["fleet", "done", "task-1-0", "cargo", "test"]).is_ok());
        assert!(Cli::try_parse_from(["fleet", "done", "task-1-0"]).is_err(), "no check");
        assert!(Cli::try_parse_from(["fleet", "done"]).is_err(), "no block");
    }

    /// The same rejoin contract the message verbs have: a model that writes the
    /// check unquoted must not have it truncated at the first space. The flags
    /// inside a real criterion (`-p parser`, `--noEmit`) have to survive too, which
    /// is what `trailing_var_arg` buys over `num_args(1..)`.
    #[test]
    fn the_check_command_survives_however_the_model_quotes_it() {
        for argv in [
            vec!["fleet", "done", "task-1-0", "cargo test -p parser --quiet"],
            vec!["fleet", "done", "task-1-0", "cargo", "test", "-p", "parser", "--quiet"],
        ] {
            let cli = Cli::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            let Command::Done { task, check } = cli.command else { panic!("expected done") };
            assert_eq!(task, "task-1-0");
            assert_eq!(join(check), "cargo test -p parser --quiet", "{argv:?}");
        }
    }

    /// **The delivery contract, unmoved.** `fleet done` runs a check that may fail,
    /// and the CLI's exit code must still describe only whether the receipt
    /// reached a live terminal. `report` is the single place that decides, and
    /// `done` reaches it through the ordinary `Op::Send` path — so a failing check
    /// with a delivered receipt is exit 0, and a passing check whose receipt was
    /// refused is exit 1.
    #[test]
    fn a_receipts_exit_code_describes_delivery_and_never_the_check() {
        let failing = done::op(PaneId::Worker(2), "task-1-0", "exit 9").expect("a receipt");
        let Op::Send { text, .. } = failing else { panic!("expected a send") };
        assert!(text.contains("· exit 9"), "the check's failure is in the body: {text}");

        assert!(
            report(OpResult::Delivered { msg_id: "msg-9".into(), accepted: true, detail: None }, false),
            "a delivered receipt exits zero however the check went",
        );
        assert!(
            !report(
                OpResult::Delivered {
                    msg_id: "msg-9".into(),
                    accepted: false,
                    detail: Some("orch is dead".into())
                },
                false
            ),
            "an undelivered receipt exits non-zero however the check went",
        );
    }

    // --- the handoff (WP-13) ----------------------------------------------------

    /// A handoff has to carry a claim and a way to check it. Both are `required`
    /// in clap, not documented and hoped for — the same contract-on-the-sender
    /// argument that made `fleet cmd --why` required (D-045) and a task block's
    /// criteria required (WP-05). `--open` is not: a mission with no loose ends
    /// must not have to invent one.
    #[test]
    fn a_handoff_without_a_claim_or_evidence_is_refused_by_the_parser() {
        let complete = ["fleet", "handoff", "--built", "the parser lands", "--evidence", "cargo test"];
        assert!(Cli::try_parse_from(complete).is_ok());
        assert!(Cli::try_parse_from(["fleet", "handoff", "--evidence", "cargo test"]).is_err());
        assert!(Cli::try_parse_from(["fleet", "handoff", "--built", "it is done"]).is_err());
        assert!(Cli::try_parse_from(["fleet", "handoff"]).is_err());
    }

    /// Evidence and loose ends repeat and keep their order, for the reason the
    /// criteria flags do: a mission worth handing over has more than one way in,
    /// and joining them by hand is a step a model skips.
    #[test]
    fn the_handoff_flags_repeat_and_keep_their_order() {
        let cli = Cli::try_parse_from([
            "fleet", "handoff", "--built", "the parser accepts nested groups",
            "--evidence", "cargo test -p parser", "--evidence", "fleet/integration @ a1b2c3d",
            "--open", "the error messages are still the tokenizer's",
        ])
        .expect("a complete handoff");
        let Command::Handoff { built, evidence, open } = cli.command else {
            panic!("expected handoff")
        };
        assert_eq!(built, "the parser accepts nested groups");
        assert_eq!(evidence, vec!["cargo test -p parser", "fleet/integration @ a1b2c3d"]);
        assert_eq!(open, vec!["the error messages are still the tokenizer's"]);
    }

    /// The altitude rule, enforced where the sender can read it. `handoff` is
    /// the orchestrator saying the *mission* is finished; a worker closing one
    /// block has `fleet done`, and the refusal names it rather than leaving a
    /// worker to guess which verb it wanted.
    #[test]
    fn only_orch_may_hand_the_work_back_and_a_worker_is_told_which_verb_it_wanted() {
        let evidence = vec!["cargo test".to_string()];
        assert!(handoff_op(PaneId::Orch, "it is done".into(), evidence.clone(), vec![]).is_ok());

        let why = handoff_op(PaneId::Worker(2), "it is done".into(), evidence, vec![])
            .expect_err("a worker must be refused")
            .to_string();
        assert!(why.contains("worker-2"), "{why}");
        assert!(why.contains("fleet done"), "the refusal says what to run instead: {why}");
    }

    /// A handoff never becomes a message, and never claims a pty. It answers
    /// `recorded` through the same renderer a task claim and a message to the
    /// operator use — one word, one implementation, no room to drift.
    #[test]
    fn a_handoff_is_an_op_of_its_own_and_reports_recorded() {
        let op = handoff_op(PaneId::Orch, "the CLI ships".into(), vec!["cargo test".into()], vec![])
            .expect("orch may hand the work back");
        assert!(matches!(op, Op::Handoff { .. }), "never a send, never a task: {op:?}");
        assert!(report(OpResult::Recorded { record_id: "handoff-1".into() }, false));
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

    // --- the operator as a participant (WP-07) ----------------------------------

    /// The human's name has to survive the CLI boundary like any other, or the
    /// sentence the worker brief teaches — `fleet send operator "…"` — exits 2.
    #[test]
    fn the_pane_argument_accepts_the_operator_by_name() {
        let cli = Cli::try_parse_from(["fleet", "send", "operator", "which", "schema?"]).unwrap();
        let Command::Send { pane, text } = cli.command else { panic!("expected send") };
        assert_eq!(pane.parse::<PaneId>().unwrap(), PaneId::Operator);
        assert_eq!(join(text), "which schema?");
    }

    /// The vocabulary, at the one place a model reads it. `recorded` exits zero
    /// and never prints the word `accepted`: the promise `accepted` makes is
    /// about a live pty, and there is none — a receipt that blurred them would
    /// teach the fleet that reaching a human and reaching a terminal are the
    /// same event.
    #[test]
    fn a_message_to_the_operator_reports_recorded_and_never_accepted() {
        assert!(report(OpResult::Recorded { record_id: "msg-9".into() }, false));
        // Both callers of the word share one variant, so there is one renderer
        // and it cannot drift between them.
        assert!(report(OpResult::Recorded { record_id: "task-1-0".into() }, false));
    }

    /// The operator sits on the roster with a word that is not a pane state.
    /// `—` in the context column for the same reason orch has one: nothing
    /// sampled a transcript, and a human does not have one to sample.
    #[test]
    fn the_roster_lists_the_operator_as_present_rather_than_live() {
        let lines = roster_lines(&[
            PaneEntry::new(PaneId::Operator, PaneState::Present),
            PaneEntry::new(PaneId::Orch, PaneState::Live),
        ]);
        assert!(lines[0].starts_with("operator"), "{}", lines[0]);
        assert!(lines[0].contains("present"), "{}", lines[0]);
        assert!(!lines[0].contains("live"), "never a faked liveness: {}", lines[0]);
        assert!(lines[0].contains('—'), "no gauge for a human: {}", lines[0]);
        assert!(lines[1].contains("live"), "a real pane still says live: {}", lines[1]);
    }
}
