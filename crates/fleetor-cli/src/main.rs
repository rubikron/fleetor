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
use fleetor_core::task::{ChainEntry, Kind, TaskRecord, TaskStatus, TASK_STATUSES};
use fleetor_core::wire::{Hello, Op, OpResult, TaskAction};
use fleetor_ipc::{Client, UnixTransport};
use std::collections::HashSet;
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
    /// Goals and tasks: open, list, show and update them (D-100).
    ///
    /// One verb with subcommands, so `task` is one entry in `brief::VERBS`.
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
        /// The goal this closes, by number. Its chain gets the handoff and a
        /// list of its tasks still open.
        #[arg(long, value_name = "NUMBER")]
        goal: Option<String>,
    },
    /// Who exists and whether they are live.
    Roster,
    /// Your own pane name.
    Whoami,
}

/// What `fleet task` does. Flat flags, never JSON.
#[derive(Subcommand)]
enum TaskCmd {
    /// Open a goal (`--goal`) or a task under one (`--parent <goal number>`).
    ///
    /// Opening a task assigns nobody — send the worker its job with `fleet send`.
    Post {
        /// Open a goal instead of a task. Takes no value.
        #[arg(long, num_args = 0..=1, value_name = "")]
        goal: Option<Option<String>>,
        /// The task's owner: a worker as `2` / `worker-2`. Leave out for unowned.
        #[arg(long, value_name = "PANE")]
        to: Option<String>,
        /// What this enables when it is done.
        #[arg(long, required = true)]
        outcome: String,
        /// A technical check anyone could run. Repeatable; a task needs one.
        #[arg(long = "crit-t", action = clap::ArgAction::Append, value_name = "CHECK")]
        crit_t: Vec<String>,
        /// Which part of the vision this serves. Repeatable; required.
        #[arg(long = "crit-s", action = clap::ArgAction::Append, value_name = "VISION")]
        crit_s: Vec<String>,
        /// Detail the owner needs that does not fit in the outcome.
        #[arg(long)]
        instructions: Option<String>,
        /// The goal this task serves, by number.
        #[arg(long, value_name = "NUMBER")]
        parent: Option<String>,
        /// The task this stream of work comes back together in, by number.
        #[arg(long = "converges-on", value_name = "NUMBER")]
        converges_on: Option<String>,
        /// Who reviews it: a worker as `3` / `worker-3`. Never its owner.
        #[arg(long, value_name = "PANE")]
        reviewer: Option<String>,
    },
    /// Change a task's status. `in-progress` takes it up and makes you its owner;
    /// only the owner may say `done`.
    Update {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        /// One of `planned`, `in-progress`, `done`, `dropped`.
        #[arg(long)]
        status: Option<String>,
        /// Why, or what you checked.
        #[arg(long)]
        note: Option<String>,
    },
    /// Put a finding or progress on a task. Changes neither status nor owner.
    Comment {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        #[arg(trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Replace a task's outcome or criteria. Each flag replaces that whole
    /// field, so restate every criterion you want kept. The old text stays in
    /// the chain. Only the creator, orch (a worker's task) or the operator may.
    Edit {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        #[arg(long)]
        outcome: Option<String>,
        /// The full new list of technical criteria. Repeatable.
        #[arg(long = "crit-t", action = clap::ArgAction::Append, value_name = "CHECK")]
        crit_t: Vec<String>,
        /// The full new list of vision criteria. Repeatable.
        #[arg(long = "crit-s", action = clap::ArgAction::Append, value_name = "VISION")]
        crit_s: Vec<String>,
        /// Name the reviewer. orch and the operator only, on any task.
        #[arg(long, value_name = "PANE")]
        reviewer: Option<String>,
        /// Put the task under this goal, by number. orch and the operator only.
        #[arg(long, value_name = "NUMBER")]
        parent: Option<String>,
    },
    /// Say a task is not worth doing, and why. Anyone may; it changes neither
    /// status nor owner, and the task list shows how many have flagged it.
    Flag {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        #[arg(trailing_var_arg = true)]
        reason: Vec<String>,
    },
    /// Take a destructive or counterproductive task off the board, and say why.
    /// Its chain is kept and `restore` undoes it. orch and the operator only;
    /// orch never the operator's tasks.
    Remove {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        #[arg(trailing_var_arg = true)]
        reason: Vec<String>,
    },
    /// Undo a removal: the task returns with the status and owner it had.
    Restore {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        #[arg(trailing_var_arg = true)]
        reason: Vec<String>,
    },
    /// Put your verdict on a task's work. Not for the task's owner. It changes
    /// neither status nor owner.
    Review {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        /// The criteria are met. Takes no value.
        #[arg(long)]
        met: bool,
        /// They are not, and why: which criterion, and what you saw.
        #[arg(long = "not-met", value_name = "REASON")]
        not_met: Option<String>,
        /// What you checked, with `--met`.
        #[arg(long)]
        note: Option<String>,
    },
    /// Hand on a task you cannot finish. It returns to planned with no owner,
    /// and the next agent starts from these four fields.
    Release {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
        /// Why you are stopping.
        #[arg(long)]
        why: Option<String>,
        /// What is finished.
        #[arg(long)]
        done: Option<String>,
        /// What remains.
        #[arg(long)]
        left: Option<String>,
        /// Branch and commit the work sits at. Filled from your checkout when
        /// you are the owner; required otherwise.
        #[arg(long = "where", value_name = "BRANCH @ COMMIT")]
        place: Option<String>,
    },
    /// One goal or task with its criteria and its whole chain.
    Show {
        /// The task's number — `14`, not `#14`.
        #[arg(value_name = "NUMBER")]
        task: Option<String>,
    },
    /// Every goal and task, one line each, tasks indented under their goal.
    List {
        /// Show criteria, instructions and each chain too.
        #[arg(long)]
        full: bool,
        /// Only `planned` and `in-progress`.
        #[arg(long)]
        open: bool,
        /// Only the tasks you own.
        #[arg(long)]
        mine: bool,
        /// Only removed tasks, which the list otherwise hides.
        #[arg(long)]
        removed: bool,
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
    let full = matches!(
        cli.command,
        Command::Task { action: TaskCmd::List { full: true, .. } | TaskCmd::Show { .. } }
    );
    let filter = match cli.command {
        Command::Task { action: TaskCmd::List { open, mine, removed, .. } } => Filter {
            open,
            mine: if mine { Some(me()?) } else { None },
            removed: Some(removed),
        },
        _ => Filter::default(),
    };

    // `whoami` answers from the environment alone: it is the first thing a
    // confused pane tries, and it must work even when the hub is down.
    if let Command::Whoami = cli.command {
        println!("{}", me()?);
        return Ok(ExitCode::SUCCESS);
    }

    // A receipt is recorded on its task after the message has gone.
    let mut receipt = None;
    let op = match cli.command {
        Command::Send { pane, text } => Op::Send {
            to: pane.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}"))?,
            text: join(text),
        },
        Command::Broadcast { text } => Op::Broadcast { text: broadcast_text(text)? },
        Command::Reply { text } => Op::Reply { text: reply_text(text)? },
        Command::Cmd { pane, command, why } => {
            Op::Cmd { to: target(&pane)?, command: join(command), why }
        }
        Command::Task { action } => Op::Task { action: task_action(action)? },
        // The check runs *before* the connection is opened — a receipt describes
        // something that already happened, and a socket held open for the length
        // of a test suite is a connection doing nothing but waiting.
        Command::Done { task, check } => {
            let (op, place, check) = done::checked(me()?, &task, &join(check))?;
            receipt = Some((task, place, check));
            op
        }
        Command::Handoff { built, evidence, open, goal } => {
            let goal = goal.map(|raw| number(&raw, "--goal")).transpose()?;
            let Op::Handoff { built, evidence, open, .. } = handoff_op(me()?, built, evidence, open)? else {
                unreachable!("handoff_op builds a handoff")
            };
            Op::Handoff { built, evidence, open, goal }
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
        let result = client.call(op).await?;
        // The exit code still means delivery only, so a failed record warns.
        if let Some((task, place, check)) = receipt {
            let accepted = matches!(result, OpResult::Delivered { accepted: true, .. });
            match receipt_action(&task, place, check, accepted) {
                Ok(action) => match client.call(Op::Task { action }).await {
                    Ok(OpResult::Recorded { .. }) => {}
                    Ok(OpResult::Error { message }) => eprintln!("fleet: warning — the receipt was sent but not recorded on the task: {message}"),
                    Ok(_) => {}
                    Err(e) => eprintln!("fleet: warning — the receipt was sent but not recorded on the task: {e:#}"),
                },
                Err(e) => eprintln!("fleet: warning — the receipt was sent but not recorded on a task: {e}"),
            }
        }
        anyhow::Ok(result)
    })?;

    Ok(if report(filter.apply(result), full) { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// The `fleet task` wire payload. Parsed here so a typo costs a local error
/// on the model's own stderr rather than a socket round trip.
fn task_action(action: TaskCmd) -> Result<TaskAction> {
    Ok(match action {
        TaskCmd::Post { goal, to, outcome, crit_t, crit_s, instructions, parent, converges_on, reviewer } => {
            if let Some(Some(value)) = &goal {
                let n = value.trim().trim_start_matches('#');
                anyhow::bail!(
                    "--goal takes no value, it opens a goal — to open a task under goal {n}, \
                     use --parent {n}"
                );
            }
            TaskAction::Post {
                goal: goal.is_some(),
                owner: to
                    .map(|pane| pane.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}")))
                    .transpose()?,
                outcome,
                technical: crit_t,
                vision: crit_s,
                instructions,
                parent: parent.map(|raw| number(&raw, "--parent")).transpose()?,
                converges_on: converges_on.map(|raw| number(&raw, "--converges-on")).transpose()?,
                reviewer: pane(reviewer)?,
            }
        }
        TaskCmd::Update { task, status, note } => {
            let task = task_number(task, "update")?;
            let Some(status) = status else {
                anyhow::bail!(
                    "`fleet task update` needs --status ({}). To put something on the record \
                     without changing the status, use `fleet task comment {task} \"…\"`",
                    TASK_STATUSES.join("|")
                );
            };
            TaskAction::Update {
                task,
                status: TaskStatus::parse(&status).map_err(|e| anyhow::anyhow!("{e}"))?,
                note,
            }
        }
        TaskCmd::Comment { task, text } => {
            TaskAction::Comment { task: task_number(task, "comment")?, text: join(text) }
        }
        TaskCmd::Edit { task, outcome, crit_t, crit_s, reviewer, parent } => {
            let task = task_number(task, "edit")?;
            if outcome.is_none()
                && crit_t.is_empty()
                && crit_s.is_empty()
                && reviewer.is_none()
                && parent.is_none()
            {
                anyhow::bail!(
                    "`fleet task edit {task}` needs what to replace: --outcome \"…\", the \
                     whole new list of --crit-t / --crit-s, --reviewer <pane>, or --parent <goal \
                     number>"
                );
            }
            TaskAction::Edit {
                task,
                outcome,
                technical: crit_t,
                vision: crit_s,
                reviewer: pane(reviewer)?,
                parent: parent.map(|raw| number(&raw, "--parent")).transpose()?,
            }
        }
        TaskCmd::Flag { task, reason } => {
            TaskAction::Flag { task: task_number(task, "flag")?, reason: join(reason) }
        }
        TaskCmd::Remove { task, reason } => {
            TaskAction::Remove { task: task_number(task, "remove")?, reason: join(reason) }
        }
        TaskCmd::Restore { task, reason } => {
            let reason = Some(join(reason)).filter(|text| !text.trim().is_empty());
            TaskAction::Restore { task: task_number(task, "restore")?, reason }
        }
        TaskCmd::Review { task, met, not_met, note } => {
            let task = task_number(task, "review")?;
            match (met, not_met) {
                (true, None) => TaskAction::Review { task, met: true, reason: note },
                (false, Some(reason)) => TaskAction::Review { task, met: false, reason: Some(reason) },
                _ => anyhow::bail!(
                    "`fleet task review {task}` needs exactly one verdict: --met, or --not-met \
                     \"<which criterion, and what you saw>\""
                ),
            }
        }
        TaskCmd::Release { task, why, done, left, place } => {
            let task = task_number(task, "release")?;
            let missing: Vec<&str> = [("--why", &why), ("--done", &done), ("--left", &left)]
                .iter()
                .filter(|(_, text)| text.as_deref().is_none_or(|t| t.trim().is_empty()))
                .map(|(flag, _)| *flag)
                .collect();
            if !missing.is_empty() {
                anyhow::bail!(
                    "`fleet task release {task}` needs {} — the next agent starts from what \
                     you write. --why: why you are stopping; --done: what is finished; --left: \
                     what remains",
                    missing.join(", ")
                );
            }
            TaskAction::Release {
                task,
                why: why.unwrap_or_default(),
                done: done.unwrap_or_default(),
                left: left.unwrap_or_default(),
                place,
                here: done::whereabouts(std::path::Path::new(".")),
            }
        }
        TaskCmd::Show { task } => TaskAction::Show { task: task_number(task, "show")? },
        TaskCmd::List { .. } => TaskAction::List,
    })
}

/// `fleet task list --open` / `--mine` / `--removed`: a rendering choice the
/// hub is not told.
#[derive(Default)]
struct Filter {
    open: bool,
    mine: Option<PaneId>,
    /// A list shows removed tasks or the rest, never both. `None` shows all.
    removed: Option<bool>,
}

impl Filter {
    fn apply(&self, result: OpResult) -> OpResult {
        let OpResult::Board { tasks, lineage } = result else { return result };
        let keep = |record: &TaskRecord| {
            let open = matches!(record.status, TaskStatus::Planned | TaskStatus::InProgress);
            let mine = self.mine.is_none_or(|me| {
                record.owner.as_ref().is_some_and(|o| o.pane == me && !earlier(o, lineage.as_deref()))
            });
            let removed = self.removed.is_none_or(|only| only == (record.status == TaskStatus::Removed));
            (open || !self.open) && mine && removed
        };
        OpResult::Board { tasks: tasks.into_iter().filter(keep).collect(), lineage }
    }
}

/// A task number as a model types it: `14` or a quoted `#14`.
fn number(raw: &str, what: &str) -> Result<u64> {
    raw.trim().trim_start_matches('#').parse().map_err(|_| {
        anyhow::anyhow!("{what} takes a task number like 14, not {raw:?} — `fleet task list` shows them")
    })
}

/// An optional pane argument, as `3` or `worker-3`.
fn pane(raw: Option<String>) -> Result<Option<PaneId>> {
    raw.map(|pane| pane.parse::<PaneId>().map_err(|e| anyhow::anyhow!("{e}"))).transpose()
}

/// What `fleet done` puts on the task's chain once its message has gone.
fn receipt_action(task: &str, place: done::Place, check: done::Check, accepted: bool) -> Result<TaskAction> {
    Ok(TaskAction::Receipt {
        task: number(task, "`fleet done`")?,
        check: check.command,
        status: check.status,
        branch: place.branch,
        commit: place.commit,
        uncommitted: place.dirty,
        accepted,
    })
}

/// The positional number. An unquoted `#14` never arrives: the shell reads `#`
/// as the start of a comment, so a missing number says so.
fn task_number(raw: Option<String>, sub: &str) -> Result<u64> {
    let Some(raw) = raw else {
        anyhow::bail!(
            "`fleet task {sub}` needs a task number and none arrived. If you typed `#14`, the \
             shell read `#` as the start of a comment and dropped it — type the bare number: \
             `fleet task {sub} 14`"
        );
    };
    number(&raw, &format!("`fleet task {sub}`"))
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
    Ok(Op::Handoff { built, evidence, open, goal: None })
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
        OpResult::Board { tasks, lineage } => {
            for line in board_lines(&tasks, lineage.as_deref(), full) {
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

/// One line per goal or task, tasks indented under their goal; `full` adds
/// criteria, instructions and the chain. Pure, so the shape is testable.
fn board_lines(tasks: &[TaskRecord], lineage: Option<&str>, full: bool) -> Vec<String> {
    if tasks.is_empty() {
        return vec![
            "there are no goals or tasks yet — `fleet task post --goal --outcome \"…\" \
             --crit-s \"…\"` opens the first goal"
                .to_string(),
        ];
    }
    let mut lines = Vec::new();
    for (index, depth) in tree_order(tasks) {
        let record = &tasks[index];
        let indent = "  ".repeat(depth);
        lines.push(format!("{indent}{}", summary(record, lineage)));
        if full {
            lines.extend(details(record).into_iter().map(|line| format!("{indent}    {line}")));
        }
    }
    lines
}

/// An owner from another lineage is a pane that no longer exists.
fn earlier(owner: &fleetor_core::task::Owner, lineage: Option<&str>) -> bool {
    lineage.is_some_and(|lineage| owner.lineage != lineage)
}

/// `#14 [in-progress] worker-2 — the parser accepts nested groups`.
fn summary(record: &TaskRecord, lineage: Option<&str>) -> String {
    let n = record.number;
    let flagged = match record.flaggers() {
        0 => String::new(),
        count => format!("  (flagged by {count})"),
    };
    let who = match (&record.block.kind, &record.owner) {
        (Kind::Goal, _) => {
            return format!("#{n} goal [{}] — {}{flagged}", record.status, record.block.outcome)
        }
        (Kind::Task, Some(owner)) if earlier(owner, lineage) => format!("{}, earlier run", owner.pane),
        (Kind::Task, Some(owner)) => owner.pane.to_string(),
        (Kind::Task, None) => "unowned".to_string(),
    };
    let converges = record
        .block
        .converges_on
        .map(|n| format!("  → converges on #{n}"))
        .unwrap_or_default();
    format!("#{n} [{}] {who} — {}{converges}{flagged}", record.status, record.block.outcome)
}

fn details(record: &TaskRecord) -> Vec<String> {
    let mut lines: Vec<String> =
        record.block.technical.iter().map(|c| format!("technical: {c}")).collect();
    lines.extend(record.block.vision.iter().map(|c| format!("vision: {c}")));
    if let Some(reviewer) = record.block.reviewer {
        lines.push(format!("reviewer: {reviewer}"));
    }
    if let Some(instructions) = &record.block.instructions {
        lines.push(format!("instructions: {instructions}"));
    }
    lines.extend(record.chain.iter().map(|line| {
        let with = |what: String, note: &Option<String>| match note {
            Some(note) => format!("{what} — {note}"),
            None => what,
        };
        let what = match &line.entry {
            ChainEntry::Opened { .. } => "opened this".to_string(),
            ChainEntry::TakenUp { note } => with("took this up".to_string(), note),
            ChainEntry::Status { status, note } => with(format!("marked it {status}"), note),
            ChainEntry::Commented { text } => format!("commented — {text}"),
            ChainEntry::Edited { field, old, new } => format!(
                "edited {} — was: {} — now: {}",
                field.flag(),
                old.join(" | "),
                new.join(" | ")
            ),
            ChainEntry::ReviewerSet { old, new } => match old {
                Some(old) => format!("named {new} reviewer — was {old}"),
                None => format!("named {new} reviewer"),
            },
            ChainEntry::Reviewed { met, reason, requested } => with(
                format!(
                    "reviewed it{}: {}",
                    if *requested { "" } else { " (unrequested)" },
                    if *met { "met" } else { "not met" }
                ),
                reason,
            ),
            ChainEntry::Flagged { reason } => format!("flagged it as not worth doing — {reason}"),
            ChainEntry::Removed { reason } => format!("removed it — {reason}"),
            ChainEntry::Restored { reason } => with("restored it".to_string(), reason),
            ChainEntry::Attached { old, new } => match old {
                Some(old) => format!("moved it under #{new} — was under #{old}"),
                None => format!("put it under #{new}"),
            },
            ChainEntry::Handoff { built, evidence, open, open_tasks } => format!(
                "handed off — built: {built} — evidence: {}{}{}",
                evidence.join(" | "),
                if open.is_empty() { String::new() } else { format!(" — open: {}", open.join(" | ")) },
                if open_tasks.is_empty() {
                    String::new()
                } else {
                    format!(
                        " — tasks still open: {}",
                        open_tasks.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(" ")
                    )
                }
            ),
            ChainEntry::Receipt { check, status, branch, commit, uncommitted, accepted } => format!(
                "ran `{check}` — {status} — {} @ {}{}{}",
                branch.as_deref().unwrap_or("no branch"),
                commit.as_deref().unwrap_or("no commit"),
                if *uncommitted { " + uncommitted changes" } else { "" },
                if *accepted { "" } else { " — the receipt message was not delivered" }
            ),
            ChainEntry::Released { why, done, left, place, on_behalf_of } => format!(
                "released this{} — why: {why} — done: {done} — left: {left} — where: {place}",
                on_behalf_of.map(|owner| format!(" on behalf of {owner}")).unwrap_or_default()
            ),
        };
        format!("{}: {what}", line.from)
    }));
    lines
}

/// `(index, depth)` in render order: each goal, then the tasks that name it.
/// A task whose parent is not in the list renders at the top level.
fn tree_order(tasks: &[TaskRecord]) -> Vec<(usize, usize)> {
    let numbers: HashSet<u64> = tasks.iter().map(|record| record.number).collect();
    let child_of = |record: &TaskRecord| {
        record.block.parent.filter(|parent| numbers.contains(parent) && *parent != record.number)
    };
    let mut out = Vec::new();
    for (index, record) in tasks.iter().enumerate() {
        if child_of(record).is_some() {
            continue;
        }
        out.push((index, 0));
        out.extend(
            tasks
                .iter()
                .enumerate()
                .filter(|(_, child)| child_of(child) == Some(record.number))
                .map(|(child, _)| (child, 1)),
        );
    }
    out
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

/// The body of a `fleet broadcast`, or a refusal if the pane wrote a recipient
/// where the body starts (D-078).
///
/// The same argv shape as [`reply_text`], and caught for the same reason — but
/// **the cost of missing it is not the same, and that is worth being honest
/// about.** A misaddressed `reply` is a *misroute*: the wrong pane gets the
/// message and the right one never does, which is what produced nine silent
/// misdeliveries in one measured run. A `broadcast` has no recipient to get
/// wrong; everyone receives it either way, so the cost of the unfixed defect is
/// only a stray leading word. No measurement says it has ever happened.
///
/// It is caught anyway because the *other* error is the expensive one:
/// `fleet broadcast worker-3 "…"` from a pane that meant to reach one peer
/// tells all four, which is the failure `prompts/worker.md` already calls
/// "almost never the right call". Announcing to the whole fleet what belonged
/// in one message is worse than a stray word.
///
/// **The false positive this creates, and why it is affordable.** Unlike
/// `reply`, a broadcast plausibly *does* open with a peer's name — "worker-3 is
/// blocked on the schema" is an ordinary announcement, and a pane that types it
/// unquoted is refused here for writing correct prose. So the refusal names
/// both remedies rather than assuming which was meant: send to one pane, or
/// quote the message. Either correction is one line, in the turn the pane is
/// already in.
fn broadcast_text(text: Vec<String>) -> Result<String> {
    if let Some(first) = text.first() {
        if first.parse::<PaneId>().is_ok() {
            anyhow::bail!(
                "`fleet broadcast` does not take a recipient — use \
                 `fleet send {first} \"<text>\"` to reach one pane, or quote the \
                 whole message if it really begins with a name"
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
    #[test]
    fn the_refusal_is_exactly_as_wide_as_the_recipient_fleet_send_accepts() {
        for candidate in [
            "orch", "lead", "o", "operator", "2", "w2", "worker2", "worker-2", "self",
            "w", "worker", "worker-", "worker_3", "256", "3pm", "Agreed", "annex", "orch,",
            "worker-3 said the annex is out",
        ] {
            let is_a_pane_name = candidate.parse::<PaneId>().is_ok();
            let refused = reply_from(&["fleet", "reply", candidate, "text"]).is_err();
            assert_eq!(refused, is_a_pane_name, "{candidate:?}");
        }
    }

    // --- `fleet broadcast` takes no recipient either (D-078) -------------------

    /// [`reply_from`]'s twin, and through clap for the same reason: the whole
    /// discriminator is how argv was split.
    fn broadcast_from(argv: &[&str]) -> Result<String> {
        let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        let Command::Broadcast { text } = cli.command else { panic!("expected broadcast") };
        broadcast_text(text)
    }

    /// `broadcast` has the identical argv shape to `reply` — `trailing_var_arg`,
    /// no recipient field — so a name in front is glued to the body the same way.
    /// The failure it prevents is the opposite one: not a message reaching the
    /// wrong pane, but a message meant for one pane reaching all four.
    #[test]
    fn a_broadcast_that_names_a_recipient_is_refused_for_every_spelling_send_accepts() {
        for name in
            ["orch", "orchestrator", "lead", "o", "operator", "0", "3", "255", "w3",
             "worker3", "worker-3", "OrCh", "Worker-3", " orch "]
        {
            assert!(
                name.parse::<PaneId>().is_ok(),
                "{name:?}: this test is only meaningful for names `fleet send` accepts"
            );
            assert!(
                broadcast_from(&["fleet", "broadcast", name, "the", "annex", "is", "out"]).is_err(),
                "{name:?} was accepted as the start of a broadcast body"
            );
        }
    }

    /// Pinned exactly, and it names **both** remedies rather than one. A
    /// broadcast plausibly does open with a peer's name, unlike a reply, so the
    /// refused pane may have meant either "send this to worker-3" or "announce
    /// that worker-3 is blocked" — and it must be able to act without guessing
    /// which reading the CLI had.
    #[test]
    fn the_broadcast_refusal_names_both_remedies() {
        assert_eq!(
            broadcast_from(&["fleet", "broadcast", "worker-3", "is blocked"])
                .unwrap_err()
                .to_string(),
            "`fleet broadcast` does not take a recipient — use `fleet send worker-3 \"<text>\"` \
             to reach one pane, or quote the whole message if it really begins with a name"
        );
    }

    /// The affordable false positive, made explicit: an announcement that really
    /// does start with a peer's name still goes out — quoted.
    #[test]
    fn a_broadcast_may_begin_with_a_peers_name_when_the_name_is_prose() {
        for (argv, expected) in [
            (
                vec!["fleet", "broadcast", "worker-3 is blocked on the schema"],
                "worker-3 is blocked on the schema",
            ),
            (vec!["fleet", "broadcast", "orch has the lock"], "orch has the lock"),
            (vec!["fleet", "broadcast", "heads", "up", "worker-3"], "heads up worker-3"),
        ] {
            assert_eq!(broadcast_from(&argv).unwrap(), expected, "{argv:?}");
        }
    }

    /// Both verbs ask `PaneId` rather than matching spellings of their own, so
    /// neither can drift from `fleet send`'s recipient or from each other.
    #[test]
    fn broadcast_and_reply_refuse_exactly_the_same_set_of_names() {
        for candidate in [
            "orch", "lead", "o", "operator", "2", "w2", "worker2", "worker-2", "self",
            "w", "worker", "worker-", "worker_3", "256", "3pm", "Agreed", "annex", "orch,",
            "worker-3 is blocked on the schema",
        ] {
            let is_a_pane_name = candidate.parse::<PaneId>().is_ok();
            let broadcast_refused =
                broadcast_from(&["fleet", "broadcast", candidate, "text"]).is_err();
            let reply_refused = reply_from(&["fleet", "reply", candidate, "text"]).is_err();
            assert_eq!(broadcast_refused, is_a_pane_name, "{candidate:?}");
            assert_eq!(broadcast_refused, reply_refused, "{candidate:?}: the two verbs disagree");
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

    // --- goals and tasks (D-100) -----------------------------------------------

    use fleetor_core::task::{board, TaskBlock};

    fn action(argv: &[&str]) -> Result<TaskAction> {
        let cli = Cli::try_parse_from(argv).map_err(|e| anyhow::anyhow!("{e}"))?;
        let Command::Task { action } = cli.command else { panic!("expected task") };
        task_action(action)
    }

    /// Goal #1 with tasks #2 (owned by worker-2, taken up) and #3 (unowned).
    fn records() -> Vec<TaskRecord> {
        let crit = |s: &str| vec![s.to_string()];
        let goal = TaskBlock::new(Kind::Goal, "one grammar", &[], &crit("the parser is the product"), None, None, None, None).unwrap();
        let task = |owner, outcome: &str| {
            TaskBlock::new(Kind::Task, outcome, &crit("cargo test -p parser"), &crit("one grammar"), owner, Some("start from the tokenizer"), Some(1), None).unwrap()
        };
        let at = |n, from, entry: ChainEntry| entry.into_event(n, from, "run-1", "lin-1");
        board(&[
            at(1, PaneId::Operator, ChainEntry::Opened { block: goal }),
            at(2, PaneId::Orch, ChainEntry::Opened { block: task(Some(PaneId::Worker(2)), "nested groups parse") }),
            at(3, PaneId::Orch, ChainEntry::Opened { block: task(None, "errors name the token") }),
            at(2, PaneId::Worker(2), ChainEntry::status(TaskStatus::InProgress, Some("starting"))),
            at(2, PaneId::Worker(3), ChainEntry::comment("the tokenizer leaks").unwrap()),
            at(2, PaneId::Orch, ChainEntry::Edited {
                field: fleetor_core::task::Field::Outcome,
                old: crit("nested groups parse"),
                new: crit("nested groups parse at any depth"),
            }),
        ])
    }

    #[test]
    fn goal_is_a_flag_and_a_task_names_its_goal_with_parent() {
        let TaskAction::Post { goal, owner, parent, technical, .. } =
            action(&["fleet", "task", "post", "--goal", "--outcome", "one grammar", "--crit-s", "v"]).unwrap()
        else {
            panic!("expected a post")
        };
        assert!(goal && owner.is_none() && parent.is_none() && technical.is_empty());

        let TaskAction::Post { goal, owner, parent, technical, vision, converges_on, .. } = action(&[
            "fleet", "task", "post", "--to", "worker-3", "--outcome", "the parser lands",
            "--crit-t", "cargo test -p parser", "--crit-t", "fleet task show 14",
            "--crit-s", "one grammar", "--parent", "11", "--converges-on", "#12",
        ])
        .unwrap() else {
            panic!("expected a post")
        };
        assert!(!goal);
        assert_eq!(owner, Some(PaneId::Worker(3)));
        assert_eq!((parent, converges_on), (Some(11), Some(12)));
        assert_eq!(technical, vec!["cargo test -p parser", "fleet task show 14"]);
        assert_eq!(vision, vec!["one grammar"]);

        let TaskAction::Post { owner, .. } =
            action(&["fleet", "task", "post", "--outcome", "o", "--crit-t", "t", "--crit-s", "s"]).unwrap()
        else {
            panic!("expected a post")
        };
        assert_eq!(owner, None, "an owner is optional");
    }

    /// `--goal 11` is never read as opening a goal.
    #[test]
    fn goal_with_a_number_is_refused_and_points_at_parent() {
        for value in ["11", "#11"] {
            let why = action(&["fleet", "task", "post", "--goal", value, "--outcome", "o", "--crit-s", "s"])
                .unwrap_err()
                .to_string();
            assert!(why.contains("to open a task under goal 11, use --parent 11"), "{why}");
        }
    }

    #[test]
    fn a_task_number_is_bare_or_a_quoted_hash() {
        for raw in ["14", "#14"] {
            assert_eq!(action(&["fleet", "task", "show", raw]).unwrap(), TaskAction::Show { task: 14 });
        }
        assert_eq!(
            action(&["fleet", "task", "update", "14", "--status", "in-progress", "--note", "on it"]).unwrap(),
            TaskAction::Update { task: 14, status: TaskStatus::InProgress, note: Some("on it".into()) },
        );
        let why = action(&["fleet", "task", "show", "task-1-0"]).unwrap_err().to_string();
        assert!(why.contains("task number like 14"), "{why}");
    }

    /// An unquoted `#14` is eaten by the shell, so the subcommand sees nothing.
    #[test]
    fn a_missing_number_explains_the_shell_comment() {
        for argv in [vec!["fleet", "task", "show"], vec!["fleet", "task", "update", "--status", "done"]] {
            let why = action(&argv).unwrap_err().to_string();
            assert!(why.contains("`#` as the start of a comment"), "{why}");
            assert!(why.contains(&format!("`fleet task {} 14`", argv[2])), "{why}");
        }
    }

    #[test]
    fn a_status_is_parsed_before_the_wire_and_a_typo_names_the_four() {
        let why = action(&["fleet", "task", "update", "14", "--status", "claimed"]).unwrap_err().to_string();
        for status in fleetor_core::task::TASK_STATUSES {
            assert!(why.contains(status), "the refusal names {status}: {why}");
        }
        let why = action(&["fleet", "task", "update", "14", "--note", "x"]).unwrap_err().to_string();
        assert!(why.contains("--status") && why.contains("fleet task comment 14"), "{why}");
    }

    /// A verdict is exactly one of `--met` and `--not-met "<reason>"`, and the
    /// reviewer is named on `post` and `edit` by pane.
    #[test]
    fn a_review_takes_one_verdict_and_the_reviewer_is_named_by_pane() {
        for argv in [
            vec!["fleet", "task", "review", "14"],
            vec!["fleet", "task", "review", "14", "--met", "--not-met", "x"],
        ] {
            let why = action(&argv).unwrap_err().to_string();
            assert!(why.contains("exactly one verdict"), "{argv:?}: {why}");
        }
        assert_eq!(
            action(&["fleet", "task", "review", "14", "--met", "--note", "ran it"]).unwrap(),
            TaskAction::Review { task: 14, met: true, reason: Some("ran it".into()) }
        );
        assert_eq!(
            action(&["fleet", "task", "review", "14", "--not-met", "depth 3 fails"]).unwrap(),
            TaskAction::Review { task: 14, met: false, reason: Some("depth 3 fails".into()) }
        );
        let TaskAction::Edit { reviewer: Some(PaneId::Worker(3)), outcome: None, .. } =
            action(&["fleet", "task", "edit", "14", "--reviewer", "3"]).unwrap()
        else {
            panic!("an edit may name only the reviewer")
        };
        let post = ["fleet", "task", "post", "--parent", "1", "--outcome", "o", "--crit-t", "t", "--crit-s", "s", "--reviewer", "worker-3"];
        let TaskAction::Post { reviewer: Some(PaneId::Worker(3)), .. } = action(&post).unwrap() else {
            panic!("post names the reviewer")
        };
    }

    /// The receipt's facts go on the chain as they were sent; a task argument
    /// that is not a number cannot be recorded, and says so.
    #[test]
    fn a_receipt_is_recorded_with_the_facts_it_was_sent_with() {
        let (_, place, check) = done::checked(PaneId::Worker(2), "#14", "exit 9").expect("a receipt");
        let TaskAction::Receipt { task: 14, check: command, status, accepted: false, .. } =
            receipt_action("#14", place.clone(), check.clone(), false).unwrap()
        else {
            panic!("a receipt action for #14")
        };
        assert_eq!((command.as_str(), status.as_str()), ("exit 9", "exit 9"));
        let why = receipt_action("the parser", place, check, true).unwrap_err().to_string();
        assert!(why.contains("task number like 14"), "{why}");
    }

    /// The three typed fields are checked before the socket; "where" is the
    /// hub's to settle, because only it knows who the owner is.
    #[test]
    fn a_release_names_every_field_it_is_missing() {
        let why = action(&["fleet", "task", "release", "14", "--why", "out of context"])
            .unwrap_err()
            .to_string();
        assert!(why.contains("--done, --left") && !why.contains("needs --why"), "{why}");
        let why = action(&["fleet", "task", "release", "--why", "x"]).unwrap_err().to_string();
        assert!(why.contains("fleet task release"), "{why}");

        let full = ["fleet", "task", "release", "14", "--why", "a", "--done", "b", "--left", "c"];
        let TaskAction::Release { task: 14, place: None, .. } = action(&full).unwrap() else {
            panic!("a release without --where leaves it to the hub")
        };
        let typed = [&full[..], &["--where", "fleet/worker-2 @ a1b2c3d"]].concat();
        let TaskAction::Release { place: Some(place), .. } = action(&typed).unwrap() else {
            panic!("--where travels as typed")
        };
        assert_eq!(place, "fleet/worker-2 @ a1b2c3d");
    }

    #[test]
    fn a_comment_is_rejoined_and_an_edit_names_whole_fields() {
        for argv in [
            vec!["fleet", "task", "comment", "14", "the tokenizer leaks"],
            vec!["fleet", "task", "comment", "14", "the", "tokenizer", "leaks"],
        ] {
            assert_eq!(
                action(&argv).unwrap(),
                TaskAction::Comment { task: 14, text: "the tokenizer leaks".into() }
            );
        }
        assert_eq!(
            action(&["fleet", "task", "edit", "#14", "--crit-t", "a", "--crit-t", "b"]).unwrap(),
            TaskAction::Edit { task: 14, outcome: None, technical: vec!["a".into(), "b".into()], vision: vec![], reviewer: None, parent: None },
        );
        let why = action(&["fleet", "task", "edit", "14"]).unwrap_err().to_string();
        assert!(why.contains("--outcome") && why.contains("whole new list"), "{why}");
        let why = action(&["fleet", "task", "comment"]).unwrap_err().to_string();
        assert!(why.contains("`fleet task comment 14`"), "{why}");
    }

    #[test]
    fn the_list_filters_to_open_and_to_mine() {
        let numbers = |filter: Filter| match filter.apply(OpResult::Board { tasks: records(), lineage: None }) {
            OpResult::Board { tasks, .. } => tasks.iter().map(|r| r.number).collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(numbers(Filter { open: true, mine: None, removed: None }), vec![1, 2, 3]);
        assert_eq!(numbers(Filter { open: false, mine: Some(PaneId::Worker(2)), removed: None }), vec![2]);
        assert_eq!(numbers(Filter { open: true, mine: Some(PaneId::Worker(3)), removed: None }), Vec::<u64>::new());
    }

    #[test]
    fn flag_remove_and_restore_carry_a_rejoined_reason_and_edit_attaches() {
        assert_eq!(
            action(&["fleet", "task", "flag", "#14", "it", "deletes", "fixtures"]).unwrap(),
            TaskAction::Flag { task: 14, reason: "it deletes fixtures".into() }
        );
        assert_eq!(
            action(&["fleet", "task", "remove", "14", "it deletes fixtures"]).unwrap(),
            TaskAction::Remove { task: 14, reason: "it deletes fixtures".into() }
        );
        assert_eq!(
            action(&["fleet", "task", "restore", "14"]).unwrap(),
            TaskAction::Restore { task: 14, reason: None }
        );
        let why = action(&["fleet", "task", "flag"]).unwrap_err().to_string();
        assert!(why.contains("`fleet task flag 14`"), "{why}");
        let TaskAction::Edit { parent, .. } = action(&["fleet", "task", "edit", "14", "--parent", "#3"]).unwrap()
        else {
            panic!("expected an edit")
        };
        assert_eq!(parent, Some(3));
        let why = TaskStatus::parse("removed").unwrap_err();
        assert!(why.contains("fleet task remove"), "{why}");
    }

    /// A list hides removed tasks unless `--removed` asks for them; `show`
    /// hides nothing.
    #[test]
    fn the_list_hides_removed_tasks_and_counts_flaggers() {
        let at = |from, entry: ChainEntry| entry.into_event(3, from, "run-1", "lin-1");
        let mut events: Vec<_> = records()
            .iter()
            .flat_map(|r| r.chain.iter().map(|l| l.entry.clone().into_event(r.number, l.from, &l.run, &l.lineage)))
            .collect();
        events.push(at(PaneId::Worker(2), ChainEntry::flag("it deletes fixtures").unwrap()));
        events.push(at(PaneId::Worker(3), ChainEntry::flag("agreed").unwrap()));
        let flagged = board(&events);
        assert!(summary(&flagged[2], None).ends_with("(flagged by 2)"), "{}", summary(&flagged[2], None));
        assert!(details(&flagged[2]).iter().any(|l| l == "worker-3: flagged it as not worth doing — agreed"));

        events.push(at(PaneId::Orch, ChainEntry::Removed { reason: "destructive".into() }));
        let numbers = |removed| {
            let filter = Filter { open: false, mine: None, removed };
            match filter.apply(OpResult::Board { tasks: board(&events), lineage: None }) {
                OpResult::Board { tasks, .. } => tasks.iter().map(|r| r.number).collect::<Vec<_>>(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(numbers(Some(false)), vec![1, 2]);
        assert_eq!(numbers(Some(true)), vec![3]);
        assert_eq!(numbers(None), vec![1, 2, 3]);
    }

    #[test]
    fn an_empty_list_says_how_to_open_the_first_goal() {
        let lines = board_lines(&[], None, false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("fleet task post --goal"), "{}", lines[0]);
    }

    #[test]
    fn the_list_is_one_line_each_with_tasks_under_their_goal() {
        assert_eq!(
            board_lines(&records(), None, false),
            vec![
                "#1 goal [planned] — one grammar",
                "  #2 [in-progress] worker-2 — nested groups parse at any depth",
                "  #3 [planned] unowned — errors name the token",
            ]
        );
    }

    /// An owner from another session is a pane that no longer exists, so it
    /// reads as available and is not `--mine`.
    #[test]
    fn an_owner_from_another_session_reads_as_an_earlier_run() {
        let lines = board_lines(&records(), Some("another-lineage"), false);
        assert_eq!(lines[1], "  #2 [in-progress] worker-2, earlier run — nested groups parse at any depth");
        let mine = Filter { open: false, mine: Some(PaneId::Worker(2)), removed: None };
        let kept = |lineage: &str| {
            match mine.apply(OpResult::Board { tasks: records(), lineage: Some(lineage.into()) }) {
                OpResult::Board { tasks, .. } => tasks.len(),
                other => panic!("{other:?}"),
            }
        };
        let own = records()[1].owner.clone().unwrap().lineage;
        assert_eq!((kept(&own), kept("another-lineage")), (1, 0));
    }

    #[test]
    fn a_task_whose_goal_is_not_listed_renders_at_the_top_level() {
        let lines = board_lines(&records()[1..2], None, false);
        assert_eq!(lines, vec!["#2 [in-progress] worker-2 — nested groups parse at any depth"]);
    }

    #[test]
    fn full_shows_the_criteria_and_the_attributed_chain() {
        let full = board_lines(&records()[1..2], None, true).join("\n");
        assert!(full.contains("technical: cargo test -p parser"), "{full}");
        assert!(full.contains("vision: one grammar"), "{full}");
        assert!(full.contains("instructions: start from the tokenizer"), "{full}");
        assert!(full.contains("orch: opened this"), "{full}");
        assert!(full.contains("worker-2: took this up — starting"), "{full}");
        assert!(full.contains("worker-3: commented — the tokenizer leaks"), "{full}");
        assert!(
            full.contains("orch: edited --outcome — was: nested groups parse — now: nested groups parse at any depth"),
            "{full}"
        );
        assert!(full.contains("worker-2 — nested groups parse at any depth"), "the summary is the new text: {full}");
    }

    #[test]
    fn a_recorded_entry_and_a_list_both_exit_zero() {
        assert!(report(OpResult::Recorded { record_id: "14".into() }, false));
        assert!(report(OpResult::Board { tasks: records(), lineage: None }, true));
        assert!(report(OpResult::Board { tasks: vec![], lineage: None }, false), "an empty list is not a failure");
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
        let Command::Handoff { built, evidence, open, .. } = cli.command else {
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
