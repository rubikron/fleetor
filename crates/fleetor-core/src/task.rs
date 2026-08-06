//! `fleet task` — the blackboard: the decomposition everyone can see (WP-05).
//!
//! **The board is a diary, not a dispatcher.** Nothing in this module, and
//! nothing that calls it, permits, orders, delays or refuses anything else the
//! fleet does. Posting a block assigns nobody: assignment travels as an ordinary
//! `fleet send`, exactly as it did before this file existed, and a `fleet send`
//! that consulted the board would be the `Assign` op that `wire.rs:16` records as
//! "the seed of the ticket system growing back". D-030 deleted that system; this
//! is deliberately the shape that cannot become it again.
//!
//! What that rules out, permanently and by design:
//!
//!  - no gate, and no enforced transition — `done → planned` is allowed, because
//!    a board that refused it would be asserting it knows better than the agent
//!    who just found out the work was not finished;
//!  - no supervisor or poller reading the board and acting on it;
//!  - no routing, ordering or load-balancing derived from task state;
//!  - no blocking claim: [`TaskStatus::Claimed`] is a sentence an agent wrote
//!    down, not a lock anything waits on.
//!
//! Every task event is therefore **a claim an agent made**, timestamped and
//! attributed (Tier 1.6: outcomes, not intentions). `done` is an *unverified*
//! claim; WP-06's receipts and peer review are what turn one into a fact.
//!
//! **Board state is [`board`], a fold over the event log** — the same replay the
//! message feed uses, with no second table and no second source of truth. Two
//! processes reading the same log compute the same board, and a hub restart
//! forgets nothing.

use crate::event::FleetEvent;
use crate::pane::PaneId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The four statuses, in the order the briefs teach them — the list a refusal
/// prints back at the model that typed something else.
pub const TASK_STATUSES: [&str; 4] = ["planned", "claimed", "done", "dropped"];

/// What someone *says* a block's state is.
///
/// Descriptive, never enforced. No code anywhere checks that a transition is
/// legal, because there is no legal set: a worker who marks a block `done` and
/// then finds a failing criterion must be able to say `claimed` again, and a
/// board that argued with them would only teach the fleet to stop updating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    /// Written down, nobody has said they picked it up.
    Planned,
    /// Someone said they are on it. **Not a lock** — nothing waits on this.
    Claimed,
    /// Someone claims the criteria are met. Unverified until WP-06's review.
    Done,
    /// Someone decided this should not be built. The block stays on the board
    /// with its note, because *why* a slice was dropped is the part worth keeping.
    Dropped,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Planned => "planned",
            TaskStatus::Claimed => "claimed",
            TaskStatus::Done => "done",
            TaskStatus::Dropped => "dropped",
        }
    }

    /// Parse what a model typed. The refusal names the four words, because the
    /// model reads its own stderr and self-corrects from it.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "planned" => Ok(TaskStatus::Planned),
            "claimed" => Ok(TaskStatus::Claimed),
            "done" => Ok(TaskStatus::Done),
            "dropped" => Ok(TaskStatus::Dropped),
            other => Err(format!(
                "{other:?} is not a task status — the board uses {}",
                TASK_STATUSES.join(", ")
            )),
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One task block, in the vision's own shape.
///
/// The field list is not a schema someone designed here — it is what the vision
/// says a slice of work has to carry to be worth cutting: what it *enables*
/// ([`outcome`](Self::outcome)), how anyone could check it
/// ([`technical`](Self::technical)), which part of the confirmed vision it serves
/// ([`semantic`](Self::semantic)), who is doing it ([`worker`](Self::worker)),
/// and where it sits among the other streams ([`parent`](Self::parent),
/// [`converges_on`](Self::converges_on)).
///
/// The block carries no id: the id lives on the [`FleetEvent::Task`] that posted
/// it, so an update event and its post are joined on one field rather than two
/// spellings of the same string that could disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBlock {
    /// What this execution *enables* — not the activity, the change it makes
    /// possible.
    pub outcome: String,
    /// Checkable technical criteria, command-shaped where possible. Plural
    /// because a slice worth cutting usually has more than one.
    pub technical: Vec<String>,
    /// Which part of the confirmed vision this serves. The link back, without
    /// which a block is a chore rather than a slice of the thing being built.
    pub semantic: Vec<String>,
    /// The pane whose job this is. Recorded here so the board reads as a
    /// decomposition that has already been decided — **not** consulted by
    /// anything at delivery time.
    pub worker: PaneId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// The block this one was cut out of, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The block this stream of work comes back together in, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub converges_on: Option<String>,
}

impl TaskBlock {
    /// Build a block, or say why it is not one.
    ///
    /// This is validation at the system boundary, and it is the *only* checking
    /// this file does. It refuses a block with nothing checkable in it — an
    /// outcome with no criteria is a wish, and the whole point of the shape is
    /// that a claim of `done` can be argued with. It does not, and must never,
    /// check anything about the *fleet*: whether the worker exists, whether it is
    /// busy, whether the parent id resolves. Those are the questions a dispatcher
    /// asks.
    pub fn new(
        outcome: &str,
        technical: &[String],
        semantic: &[String],
        worker: PaneId,
        instructions: Option<&str>,
        parent: Option<&str>,
        converges_on: Option<&str>,
    ) -> Result<Self, String> {
        let outcome = required(outcome, "--outcome", "what this block enables when it is done")?;
        let technical = criteria(
            technical,
            "--crit-t",
            "a check someone else could run — `cargo test -p parser`, not \"works well\"",
        )?;
        let semantic =
            criteria(semantic, "--crit-s", "which part of the confirmed vision this block serves")?;
        Ok(Self {
            outcome,
            technical,
            semantic,
            worker,
            instructions: optional(instructions),
            parent: optional(parent),
            converges_on: optional(converges_on),
        })
    }

    /// The log entry that puts this block on the board. `at` and `from` are what
    /// make it a claim someone made rather than a fact the system asserts.
    pub fn into_event(self, id: impl Into<String>, from: PaneId) -> FleetEvent {
        FleetEvent::Task {
            task: id.into(),
            from,
            at: crate::time::now_ms(),
            change: TaskChange::Posted { block: self },
        }
    }
}

/// A later claim about a block already on the board: a status, a note, or both.
///
/// **Anyone may post one.** There is no ownership check, deliberately — the
/// `from` field is the accountability, and a peer who spots that a block marked
/// `done` has a failing criterion must be able to say so on the same record. The
/// log shows who said what; ownership here is social, not coded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskUpdate {
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl TaskUpdate {
    /// An update has to say *something*. One with neither a status nor a note
    /// would be an attributed, timestamped record of nothing at all.
    pub fn new(task: &str, status: Option<TaskStatus>, note: Option<&str>) -> Result<Self, String> {
        let task = required(task, "<task-id>", "the block this is about — `fleet task list`")?;
        let note = optional(note);
        if status.is_none() && note.is_none() {
            return Err(format!(
                "an update needs --status ({}) or --note \"…\", or both — \
                 otherwise there is nothing on the record but the fact you spoke",
                TASK_STATUSES.join("|")
            ));
        }
        Ok(Self { task, status, note })
    }

    pub fn into_event(self, from: PaneId) -> FleetEvent {
        FleetEvent::Task {
            task: self.task,
            from,
            at: crate::time::now_ms(),
            change: TaskChange::Updated { status: self.status, note: self.note },
        }
    }
}

/// What one [`FleetEvent::Task`] says: the block went up, or something was
/// claimed about it. Internally tagged, so the DB payload and the TypeScript
/// mirror discriminate on the same `change` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum TaskChange {
    Posted { block: TaskBlock },
    Updated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<TaskStatus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

/// One claim appended after the block went up, kept in full rather than folded
/// away: a status with no reasoning is an effect whose cause was thrown out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNote {
    pub from: PaneId,
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One block as the board currently reads: what was posted, plus every claim
/// made about it since, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: String,
    pub block: TaskBlock,
    pub posted_by: PaneId,
    pub posted_at: i64,
    /// The most recent status *claimed*. `Planned` until somebody says otherwise
    /// — never inferred from anything the fleet did.
    pub status: TaskStatus,
    pub updates: Vec<TaskNote>,
}

impl TaskEntry {
    /// Fold one claim in, returning the new entry. Consuming rather than
    /// `&mut self` so a caller cannot half-apply an update to a board someone
    /// else is holding.
    fn with_update(self, note: TaskNote) -> Self {
        let status = note.status.unwrap_or(self.status);
        let updates = [self.updates, vec![note]].concat();
        Self { status, updates, ..self }
    }
}

/// Replay the board out of the event log — the same fold the message feed does,
/// which is why there is no `tasks` table.
///
/// Entries come back in the order they were posted. A `posted` for an id already
/// on the board replaces it (last write wins); an `updated` for an id that was
/// never posted is skipped, because there is no block for it to be a claim
/// *about*. The hub refuses those at accept time, so a skipped one here means a
/// log written by an older build — the same "skip the row, keep the replay"
/// posture `events_since` takes (L9).
pub fn board<'a>(events: impl IntoIterator<Item = &'a FleetEvent>) -> Vec<TaskEntry> {
    let mut order: Vec<String> = Vec::new();
    let mut entries: HashMap<String, TaskEntry> = HashMap::new();

    for event in events {
        let FleetEvent::Task { task, from, at, change } = event else { continue };
        match change {
            TaskChange::Posted { block } => {
                if !entries.contains_key(task) {
                    order.push(task.clone());
                }
                entries.insert(
                    task.clone(),
                    TaskEntry {
                        id: task.clone(),
                        block: block.clone(),
                        posted_by: *from,
                        posted_at: *at,
                        status: TaskStatus::Planned,
                        updates: Vec::new(),
                    },
                );
            }
            TaskChange::Updated { status, note } => {
                let Some(existing) = entries.remove(task) else { continue };
                let claim =
                    TaskNote { from: *from, at: *at, status: *status, note: note.clone() };
                entries.insert(task.clone(), existing.with_update(claim));
            }
        }
    }

    order.into_iter().filter_map(|id| entries.remove(&id)).collect()
}

// --- validation ---------------------------------------------------------------

/// Trim, drop what a terminal would execute rather than print, and refuse empty.
/// The hint is what to write instead, because a model reads the refusal off its
/// own stderr.
fn required(raw: &str, flag: &str, hint: &str) -> Result<String, String> {
    let text = plain(raw);
    if text.is_empty() {
        return Err(format!("{flag} cannot be empty — say {hint}"));
    }
    Ok(text)
}

fn criteria(raw: &[String], flag: &str, hint: &str) -> Result<Vec<String>, String> {
    let kept: Vec<String> = raw.iter().map(|c| plain(c)).filter(|c| !c.is_empty()).collect();
    if kept.is_empty() {
        return Err(format!(
            "{flag} is required at least once — {hint}. \
             A block with no criteria cannot be argued with, which is the whole point of one"
        ));
    }
    Ok(kept)
}

fn optional(raw: Option<&str>) -> Option<String> {
    raw.map(plain).filter(|text| !text.is_empty())
}

/// Task text is printed into a terminal by `fleet task list`, so an escape in it
/// would repaint the reader's screen instead of being read.
///
/// Deliberately **not** `message::sanitize`: that function is the pty delivery
/// boundary and this package does not touch the delivery path at all — its diff
/// there has to stay empty. Same idea, different boundary, eight lines rather
/// than a shared helper that would couple the two.
fn plain(raw: &str) -> String {
    raw.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\t' => Some(c),
            '\r' => Some('\n'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crits(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn block() -> TaskBlock {
        TaskBlock::new(
            "the parser accepts nested groups",
            &crits(&["cargo test -p parser"]),
            &crits(&["serves the \"one grammar\" part of the vision"]),
            PaneId::Worker(2),
            Some("start from the existing tokenizer"),
            None,
            None,
        )
        .expect("a complete block")
    }

    fn posted(id: &str, from: PaneId) -> FleetEvent {
        block().into_event(id, from)
    }

    fn updated(id: &str, from: PaneId, status: Option<TaskStatus>, note: Option<&str>) -> FleetEvent {
        TaskUpdate::new(id, status, note).expect("a claim").into_event(from)
    }

    /// The schema is the vision's shape verbatim, and this is the test that says
    /// so: outcome, technical criteria, the semantic link back to the vision, the
    /// worker, the optional detail, and the two tree links.
    #[test]
    fn a_block_carries_the_visions_whole_shape() {
        let b = TaskBlock::new(
            "the CLI can post a task block",
            &crits(&["cargo test -p fleetor-cli", "fleet task list shows it"]),
            &crits(&["the blackboard everyone can see"]),
            PaneId::Worker(3),
            Some("flat flags, no JSON"),
            Some("task-1-0"),
            Some("task-1-9"),
        )
        .expect("a complete block");
        assert_eq!(b.outcome, "the CLI can post a task block");
        assert_eq!(b.technical.len(), 2, "criteria are plural");
        assert_eq!(b.semantic, vec!["the blackboard everyone can see".to_string()]);
        assert_eq!(b.worker, PaneId::Worker(3));
        assert_eq!(b.instructions.as_deref(), Some("flat flags, no JSON"));
        assert_eq!(b.parent.as_deref(), Some("task-1-0"));
        assert_eq!(b.converges_on.as_deref(), Some("task-1-9"));
    }

    /// A block with an outcome and nothing checkable is a wish. The refusal has
    /// to say what to write instead — the model reads it off its own stderr.
    #[test]
    fn a_block_with_no_criteria_is_refused_with_the_flag_to_fix() {
        let why = TaskBlock::new("something good", &[], &crits(&["the vision"]), PaneId::Worker(1), None, None, None)
            .expect_err("must be refused");
        assert!(why.contains("--crit-t"), "{why}");
        assert!(why.contains("cargo test"), "the refusal shows the shape of a real criterion: {why}");

        let why = TaskBlock::new("something good", &crits(&["cargo test"]), &[], PaneId::Worker(1), None, None, None)
            .expect_err("must be refused");
        assert!(why.contains("--crit-s"), "{why}");
        assert!(why.contains("vision"), "the semantic criterion is the link back: {why}");

        let why = TaskBlock::new("  ", &crits(&["x"]), &crits(&["y"]), PaneId::Orch, None, None, None)
            .expect_err("must be refused");
        assert!(why.contains("--outcome"), "{why}");

        // Blank criteria are dropped, not counted: `--crit-t ""` is the same as
        // not passing it at all, and must refuse rather than pass one empty rule.
        assert!(TaskBlock::new("x", &crits(&["", "  "]), &crits(&["y"]), PaneId::Orch, None, None, None).is_err());
    }

    /// Optional fields left blank are absent, not empty strings — a `parent: ""`
    /// would render as a tree link to a block that does not exist.
    #[test]
    fn blank_optional_fields_become_absent_rather_than_empty() {
        let b = TaskBlock::new("x", &crits(&["t"]), &crits(&["s"]), PaneId::Orch, Some("  "), Some(""), None)
            .expect("a block");
        assert_eq!(b.instructions, None);
        assert_eq!(b.parent, None);
        assert_eq!(b.converges_on, None);
    }

    /// `fleet task list` prints these fields into a live terminal, so an escape
    /// in one would repaint the reader's screen rather than be read as text.
    /// Newlines and tabs survive — a block's instructions are prose.
    #[test]
    fn control_characters_are_dropped_from_task_text() {
        let b = TaskBlock::new(
            "keep \x1b[2Jthe parser",
            &crits(&["cargo\ttest"]),
            &crits(&["one\r\ngrammar"]),
            PaneId::Worker(1),
            None,
            None,
            None,
        )
        .expect("a block");
        assert_eq!(b.outcome, "keep [2Jthe parser");
        assert!(!b.outcome.contains('\x1b'));
        assert_eq!(b.technical[0], "cargo\ttest", "a tab is layout, not an escape");
        assert_eq!(b.semantic[0], "one\ngrammar", "CRLF collapses rather than submitting");
    }

    /// Every status a model can type, and the refusal for one it cannot.
    #[test]
    fn the_four_statuses_parse_and_anything_else_names_them() {
        for (raw, expected) in [
            ("planned", TaskStatus::Planned),
            ("CLAIMED", TaskStatus::Claimed),
            (" done ", TaskStatus::Done),
            ("dropped", TaskStatus::Dropped),
        ] {
            assert_eq!(TaskStatus::parse(raw).unwrap(), expected, "{raw}");
        }
        let why = TaskStatus::parse("in-progress").expect_err("must be refused");
        for status in TASK_STATUSES {
            assert!(why.contains(status), "the refusal names {status}: {why}");
        }
    }

    /// An update that says nothing is an attributed, timestamped record of
    /// nothing. Either half alone is fine.
    #[test]
    fn an_update_must_carry_a_status_or_a_note() {
        let why = TaskUpdate::new("task-1-0", None, None).expect_err("must be refused");
        assert!(why.contains("--status") && why.contains("--note"), "{why}");
        assert!(TaskUpdate::new("task-1-0", Some(TaskStatus::Done), None).is_ok());
        assert!(TaskUpdate::new("task-1-0", None, Some("blocked on the tokenizer")).is_ok());
        assert!(TaskUpdate::new("  ", None, Some("x")).is_err(), "an update needs a block");
    }

    /// The event is a claim: who, when, and what they said.
    #[test]
    fn a_posted_block_becomes_an_attributed_timestamped_event() {
        let event = posted("task-1-0", PaneId::Orch);
        assert_eq!(event.kind(), "task");
        let FleetEvent::Task { task, from, at, change } = event else { panic!("expected a task") };
        assert_eq!(task, "task-1-0");
        assert_eq!(from, PaneId::Orch, "the board records who made the claim");
        assert!(at > 0, "and when");
        let TaskChange::Posted { block } = change else { panic!("expected a post") };
        assert_eq!(block.worker, PaneId::Worker(2));
    }

    /// The board is a fold over the log and nothing else — no table, no second
    /// source of truth. A fresh post reads `planned` because nobody has said
    /// otherwise, never because anything inferred it from what the fleet did.
    #[test]
    fn the_board_is_replayed_from_the_event_log() {
        let log = vec![posted("task-1-0", PaneId::Orch), posted("task-1-1", PaneId::Orch)];
        let board = board(&log);
        assert_eq!(board.len(), 2);
        assert_eq!(board[0].id, "task-1-0", "entries come back in post order");
        assert_eq!(board[1].id, "task-1-1");
        assert!(board.iter().all(|e| e.status == TaskStatus::Planned));
        assert_eq!(board[0].posted_by, PaneId::Orch);
    }

    /// Updates fold in, in order, and the latest status wins — but every claim
    /// stays on the entry, because a status with no reasoning is an effect whose
    /// cause was thrown away.
    #[test]
    fn updates_fold_in_and_every_claim_survives() {
        let log = vec![
            posted("task-1-0", PaneId::Orch),
            updated("task-1-0", PaneId::Worker(2), Some(TaskStatus::Claimed), Some("starting now")),
            updated("task-1-0", PaneId::Worker(2), None, Some("the tokenizer needs a fix first")),
            updated("task-1-0", PaneId::Worker(2), Some(TaskStatus::Done), Some("cargo test passes")),
        ];
        let board = board(&log);
        assert_eq!(board.len(), 1, "updates do not create rows");
        assert_eq!(board[0].status, TaskStatus::Done);
        assert_eq!(board[0].updates.len(), 3, "the whole trail is kept");
        assert_eq!(board[0].updates[1].status, None, "a note-only claim keeps the status it found");
        assert_eq!(board[0].updates[2].note.as_deref(), Some("cargo test passes"));
    }

    /// No enforced transitions, stated as a test so nobody adds one later. A
    /// worker who marked a block `done` and then found a failing criterion has
    /// to be able to say so; a board that refused would only teach the fleet to
    /// stop updating it.
    #[test]
    fn any_status_may_follow_any_other_including_backwards() {
        let log = vec![
            posted("task-1-0", PaneId::Orch),
            updated("task-1-0", PaneId::Worker(1), Some(TaskStatus::Done), None),
            updated("task-1-0", PaneId::Worker(1), Some(TaskStatus::Claimed), Some("crit 2 fails")),
            updated("task-1-0", PaneId::Worker(1), Some(TaskStatus::Planned), None),
        ];
        assert_eq!(board(&log)[0].status, TaskStatus::Planned);
    }

    /// Ownership is social, not coded: a peer may append to a block that is not
    /// theirs, and the log names them. This is what makes WP-06's review possible
    /// without a permission system.
    #[test]
    fn anyone_may_update_a_block_and_the_log_says_who() {
        let log = vec![
            posted("task-1-0", PaneId::Orch),
            updated("task-1-0", PaneId::Worker(4), Some(TaskStatus::Dropped), Some("duplicate of 1-2")),
        ];
        let entry = &board(&log)[0];
        assert_eq!(entry.block.worker, PaneId::Worker(2), "the block is still worker-2's");
        assert_eq!(entry.updates[0].from, PaneId::Worker(4), "worker-4 said this, and it shows");
        assert_eq!(entry.status, TaskStatus::Dropped);
    }

    /// A cycle in the tree links is data, not an error. The replay does not walk
    /// them at all — validating them into a legal graph is the workflow engine
    /// this package exists not to be.
    #[test]
    fn a_cycle_in_the_tree_links_is_tolerated_by_the_replay() {
        let cyclic = |parent: &str| TaskBlock {
            parent: Some(parent.to_string()),
            ..block()
        };
        let log = vec![
            cyclic("task-1-1").into_event("task-1-0", PaneId::Orch),
            cyclic("task-1-0").into_event("task-1-1", PaneId::Orch),
        ];
        let board = board(&log);
        assert_eq!(board.len(), 2, "both blocks are on the board");
        assert_eq!(board[0].block.parent.as_deref(), Some("task-1-1"));
        assert_eq!(board[1].block.parent.as_deref(), Some("task-1-0"));
    }

    /// The log holds every kind of event; the board sees only its own.
    #[test]
    fn non_task_events_are_ignored_by_the_replay() {
        let log = vec![
            FleetEvent::Notice { level: crate::event::NoticeLevel::Info, text: "hi".into() },
            posted("task-1-0", PaneId::Orch),
            FleetEvent::Message {
                id: "msg-1".into(),
                from: PaneId::Orch,
                to: PaneId::Worker(2),
                body: "take task-1-0".into(),
                group: None,
                accepted: true,
                detail: None,
            },
        ];
        assert_eq!(board(&log).len(), 1);
    }

    /// A claim about a block that was never posted has nothing to be a claim
    /// *about*. The hub refuses these at accept time; a log written by an older
    /// build must still replay, so this skips rather than panics (L9).
    #[test]
    fn an_update_with_no_post_is_skipped_rather_than_inventing_a_block() {
        let log = vec![updated("task-9-9", PaneId::Worker(1), Some(TaskStatus::Done), None)];
        assert!(board(&log).is_empty());
    }

    #[test]
    fn a_task_event_round_trips_through_json() {
        for event in [
            posted("task-1-0", PaneId::Orch),
            updated("task-1-0", PaneId::Worker(2), Some(TaskStatus::Claimed), Some("on it")),
            updated("task-1-0", PaneId::Worker(2), None, Some("note only")),
        ] {
            let line = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<FleetEvent>(&line).unwrap(), event, "{line}");
        }
    }
}
