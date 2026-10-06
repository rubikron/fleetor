//! Goals and tasks, kept per target repository (D-100).
//!
//! **A record, not a dispatcher.** Nothing reads a task to route, assign, wake
//! or block an agent, and taking a task up is not a lock. The permission rules
//! below check who is asking against the record's creator and owner; none of
//! them consults task state.
//!
//! Every [`FleetEvent::Chain`] is one entry in a task's chain. The record
//! itself is [`board`], a fold over those entries.

use crate::event::FleetEvent;
use crate::pane::PaneId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const TASK_STATUSES: [&str; 4] = ["planned", "in-progress", "done", "dropped"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Planned,
    InProgress,
    Done,
    Dropped,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Planned => "planned",
            TaskStatus::InProgress => "in-progress",
            TaskStatus::Done => "done",
            TaskStatus::Dropped => "dropped",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "planned" => Ok(TaskStatus::Planned),
            "in-progress" => Ok(TaskStatus::InProgress),
            "done" => Ok(TaskStatus::Done),
            "dropped" => Ok(TaskStatus::Dropped),
            other => Err(format!(
                "{other:?} is not a task status — use one of {}",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Goal,
    Task,
}

/// What a goal or task says: the text its creator wrote and its links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBlock {
    pub kind: Kind,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub technical: Vec<String>,
    pub vision: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<PaneId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// The goal this task belongs to, by number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub converges_on: Option<u64>,
}

impl TaskBlock {
    /// Checks the text only. Who may open it is [`may_open`]; whether the
    /// parent exists is the hub's to answer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: Kind,
        outcome: &str,
        technical: &[String],
        vision: &[String],
        owner: Option<PaneId>,
        instructions: Option<&str>,
        parent: Option<u64>,
        converges_on: Option<u64>,
    ) -> Result<Self, String> {
        let outcome = plain(outcome);
        if outcome.is_empty() {
            return Err("--outcome cannot be empty — say what this enables when it is done".into());
        }
        let technical = kept(technical);
        let vision = kept(vision);
        if vision.is_empty() {
            return Err("--crit-s is required at least once — say which part of the vision \
                        this serves"
                .into());
        }
        match kind {
            Kind::Goal => {
                if owner.is_some() {
                    return Err("a goal has no owner — drop --to; its tasks are what get owned".into());
                }
                if parent.is_some() {
                    return Err("a goal has no parent — drop --parent, or drop --goal to open a \
                                task under that goal"
                        .into());
                }
            }
            Kind::Task => {
                if technical.is_empty() {
                    return Err("--crit-t is required at least once — a check someone else \
                                could run, like `cargo test -p parser`, not \"works well\""
                        .into());
                }
            }
        }
        Ok(Self {
            kind,
            outcome,
            technical,
            vision,
            owner,
            instructions: instructions.map(plain).filter(|text| !text.is_empty()),
            parent,
            converges_on,
        })
    }
}

/// One thing that happened to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "kebab-case")]
pub enum ChainEntry {
    Opened {
        block: TaskBlock,
    },
    /// `--status in-progress`: the author becomes the owner.
    TakenUp {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    Status {
        status: TaskStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Leaves status and owner as they were.
    Commented {
        text: String,
    },
    /// One field replaced whole. `old` and `new` are the field's full value
    /// before and after; the outcome is a one-item list.
    Edited {
        field: Field,
        old: Vec<String>,
        new: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Field {
    Outcome,
    Technical,
    Vision,
}

impl Field {
    pub fn flag(self) -> &'static str {
        match self {
            Field::Outcome => "--outcome",
            Field::Technical => "--crit-t",
            Field::Vision => "--crit-s",
        }
    }
}

impl ChainEntry {
    pub fn status(status: TaskStatus, note: Option<&str>) -> Self {
        let note = note.map(plain).filter(|text| !text.is_empty());
        match status {
            TaskStatus::InProgress => ChainEntry::TakenUp { note },
            status => ChainEntry::Status { status, note },
        }
    }

    pub fn comment(text: &str) -> Result<Self, String> {
        let text = plain(text);
        if text.is_empty() {
            return Err("a comment needs text — `fleet task comment 14 \"what you found\"`".into());
        }
        Ok(ChainEntry::Commented { text })
    }

    pub fn into_event(self, task: u64, from: PaneId, run: &str, lineage: &str) -> FleetEvent {
        FleetEvent::Chain {
            task,
            from,
            at: crate::time::now_ms(),
            run: run.to_string(),
            lineage: lineage.to_string(),
            entry: self,
        }
    }
}

/// A chain entry with who wrote it, when, and in which run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainLine {
    pub from: PaneId,
    pub at: i64,
    pub run: String,
    pub lineage: String,
    #[serde(flatten)]
    pub entry: ChainEntry,
}

/// A pane name means a different conversation in every lineage, so an owner
/// is the pane plus the run and lineage it became owner in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub pane: PaneId,
    pub run: String,
    pub lineage: String,
}

/// A goal or task as it currently reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub number: u64,
    pub block: TaskBlock,
    pub creator: PaneId,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    pub chain: Vec<ChainLine>,
}

impl TaskRecord {
    /// The creator; orch on workers' tasks; the operator on any.
    pub fn may_edit(&self, from: PaneId) -> bool {
        from == PaneId::Operator
            || from == self.creator
            || (from == PaneId::Orch && matches!(self.creator, PaneId::Worker(_)))
    }

    /// The entries a whole-field edit by `from` produces: one per field whose
    /// value actually changes. An empty list means that field was not given.
    pub fn edits(
        &self,
        from: PaneId,
        outcome: Option<&str>,
        technical: &[String],
        vision: &[String],
    ) -> Result<Vec<ChainEntry>, String> {
        let n = self.number;
        if !self.may_edit(from) {
            return Err(format!(
                "#{n} was opened by {}, and only its creator, orch (for a worker's task) or the \
                 operator may edit it — you are {from}. Say what should change with `fleet task \
                 comment {n} \"…\"`",
                self.creator
            ));
        }
        let given = [
            (Field::Outcome, outcome.map(|text| vec![text.to_string()]), vec![self.block.outcome.clone()]),
            (Field::Technical, (!technical.is_empty()).then(|| technical.to_vec()), self.block.technical.clone()),
            (Field::Vision, (!vision.is_empty()).then(|| vision.to_vec()), self.block.vision.clone()),
        ];
        let mut entries = Vec::new();
        for (field, new, old) in given {
            let Some(new) = new else { continue };
            let new = kept(&new);
            if new.is_empty() {
                return Err(format!("{} cannot be replaced with nothing — give its new text", field.flag()));
            }
            if new != old {
                entries.push(ChainEntry::Edited { field, old, new });
            }
        }
        if entries.is_empty() {
            return Err(format!(
                "that edit changes nothing — #{n} already reads that way. Pass the whole new \
                 value of --outcome, --crit-t or --crit-s (`fleet task show {n}` prints the \
                 current ones)"
            ));
        }
        Ok(entries)
    }

    /// Whether `from`, speaking in `lineage`, may set `status`.
    pub fn may_set(&self, from: PaneId, lineage: &str, status: TaskStatus) -> Result<(), String> {
        let n = self.number;
        match status {
            TaskStatus::InProgress | TaskStatus::Done if self.block.kind == Kind::Goal => Err(format!(
                "#{n} is a goal, and a goal is not taken up or marked done — take up one of its \
                 tasks (`fleet task list`)"
            )),
            TaskStatus::InProgress => Ok(()),
            TaskStatus::Done => match &self.owner {
                Some(owner) if owner.pane == from && owner.lineage == lineage => Ok(()),
                owner => Err(format!(
                    "only the owner marks a task done, and #{n}'s owner is {} — you are {from}. \
                     If this is your work, take it up first: `fleet task update {n} --status \
                     in-progress`",
                    match owner {
                        Some(owner) if owner.lineage == lineage => owner.pane.to_string(),
                        Some(owner) => format!("{}, earlier run", owner.pane),
                        None => "nobody".to_string(),
                    }
                )),
            },
            TaskStatus::Planned | TaskStatus::Dropped if self.may_edit(from) => Ok(()),
            TaskStatus::Planned | TaskStatus::Dropped => Err(format!(
                "#{n} was opened by {}, and only its creator, orch (for a worker's task) or the \
                 operator may drop or reopen it — you are {from}",
                self.creator
            )),
        }
    }
}

/// Who may open what: goals belong to orch and the operator, orch's tasks
/// must name a goal, and a worker's task starts unowned.
pub fn may_open(from: PaneId, block: &TaskBlock) -> Result<(), String> {
    match (from, block.kind) {
        (PaneId::Worker(_), Kind::Goal) => Err(format!(
            "goals are opened by orch and the operator, and you are {from} — to record work you \
             found, open a task: drop --goal"
        )),
        (PaneId::Worker(_), Kind::Task) if block.owner.is_some() => Err(
            "a task you open starts unowned — drop --to, then message the peer or orch about it"
                .to_string(),
        ),
        (PaneId::Orch, Kind::Task) if block.parent.is_none() => Err(
            "a task must serve a goal — add --parent <goal number> (`fleet task list` shows the \
             goals), or open the goal first with `fleet task post --goal`"
                .to_string(),
        ),
        _ => Ok(()),
    }
}

/// Fold chain entries into records, in the order they were opened. An entry
/// for a number that was never opened is skipped.
pub fn board<'a>(events: impl IntoIterator<Item = &'a FleetEvent>) -> Vec<TaskRecord> {
    let mut order: Vec<u64> = Vec::new();
    let mut records: HashMap<u64, TaskRecord> = HashMap::new();

    for event in events {
        let FleetEvent::Chain { task, from, at, run, lineage, entry } = event else { continue };
        let line = ChainLine {
            from: *from,
            at: *at,
            run: run.clone(),
            lineage: lineage.clone(),
            entry: entry.clone(),
        };
        let owner = |pane| Owner { pane, run: run.clone(), lineage: lineage.clone() };
        match entry {
            ChainEntry::Opened { block } => {
                if records.contains_key(task) {
                    continue;
                }
                order.push(*task);
                records.insert(
                    *task,
                    TaskRecord {
                        number: *task,
                        block: block.clone(),
                        creator: *from,
                        status: TaskStatus::Planned,
                        owner: block.owner.map(owner),
                        chain: vec![line],
                    },
                );
            }
            ChainEntry::TakenUp { .. } => {
                let Some(record) = records.get_mut(task) else { continue };
                record.status = TaskStatus::InProgress;
                record.owner = Some(owner(*from));
                record.chain.push(line);
            }
            ChainEntry::Status { status, .. } => {
                let Some(record) = records.get_mut(task) else { continue };
                record.status = *status;
                record.chain.push(line);
            }
            ChainEntry::Commented { .. } => {
                let Some(record) = records.get_mut(task) else { continue };
                record.chain.push(line);
            }
            ChainEntry::Edited { field, new, .. } => {
                let Some(record) = records.get_mut(task) else { continue };
                match field {
                    Field::Outcome => record.block.outcome = new.join("\n"),
                    Field::Technical => record.block.technical = new.clone(),
                    Field::Vision => record.block.vision = new.clone(),
                }
                record.chain.push(line);
            }
        }
    }

    order.into_iter().filter_map(|number| records.remove(&number)).collect()
}

fn kept(raw: &[String]) -> Vec<String> {
    raw.iter().map(|c| plain(c)).filter(|c| !c.is_empty()).collect()
}

/// Task text is printed into terminals, so control characters are dropped.
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

    const RUN: &str = "run-1";
    const LINEAGE: &str = "lin-1";

    fn crits(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn goal() -> TaskBlock {
        TaskBlock::new(Kind::Goal, "one grammar", &[], &crits(&["the parser is the product"]), None, None, None, None)
            .unwrap()
    }

    fn task(owner: Option<PaneId>, parent: Option<u64>) -> TaskBlock {
        TaskBlock::new(
            Kind::Task,
            "the parser accepts nested groups",
            &crits(&["cargo test -p parser"]),
            &crits(&["one grammar"]),
            owner,
            None,
            parent,
            None,
        )
        .unwrap()
    }

    fn opened(number: u64, from: PaneId, block: TaskBlock) -> FleetEvent {
        ChainEntry::Opened { block }.into_event(number, from, RUN, LINEAGE)
    }

    fn set(number: u64, from: PaneId, status: TaskStatus) -> FleetEvent {
        ChainEntry::status(status, None).into_event(number, from, RUN, LINEAGE)
    }

    #[test]
    fn a_goal_needs_only_an_outcome_and_a_vision_criterion() {
        assert_eq!(goal().technical, Vec::<String>::new());
        let why = TaskBlock::new(Kind::Goal, "x", &[], &[], None, None, None, None).unwrap_err();
        assert!(why.contains("--crit-s"), "{why}");
        let why = TaskBlock::new(Kind::Goal, "x", &[], &crits(&["v"]), Some(PaneId::Worker(1)), None, None, None)
            .unwrap_err();
        assert!(why.contains("--to"), "{why}");
        let why =
            TaskBlock::new(Kind::Goal, "x", &[], &crits(&["v"]), None, None, Some(3), None).unwrap_err();
        assert!(why.contains("--parent"), "{why}");
    }

    #[test]
    fn a_task_needs_a_technical_criterion_and_an_owner_is_optional() {
        assert_eq!(task(None, Some(1)).owner, None);
        let why = TaskBlock::new(Kind::Task, "x", &crits(&["", " "]), &crits(&["v"]), None, None, Some(1), None)
            .unwrap_err();
        assert!(why.contains("--crit-t") && why.contains("cargo test"), "{why}");
        let why = TaskBlock::new(Kind::Task, " ", &crits(&["t"]), &crits(&["v"]), None, None, Some(1), None)
            .unwrap_err();
        assert!(why.contains("--outcome"), "{why}");
    }

    #[test]
    fn control_characters_are_dropped_from_task_text() {
        let block = TaskBlock::new(
            Kind::Task,
            "keep \x1b[2Jthe parser",
            &crits(&["cargo\ttest"]),
            &crits(&["one\r\ngrammar"]),
            None,
            None,
            Some(1),
            None,
        )
        .unwrap();
        assert_eq!(block.outcome, "keep [2Jthe parser");
        assert_eq!(block.technical[0], "cargo\ttest");
        assert_eq!(block.vision[0], "one\ngrammar");
    }

    #[test]
    fn the_four_statuses_parse_and_claimed_is_gone() {
        assert_eq!(TaskStatus::parse(" In-Progress ").unwrap(), TaskStatus::InProgress);
        let why = TaskStatus::parse("claimed").unwrap_err();
        for status in TASK_STATUSES {
            assert!(why.contains(status), "the refusal names {status}: {why}");
        }
    }

    #[test]
    fn who_may_open_what() {
        assert!(may_open(PaneId::Orch, &goal()).is_ok());
        assert!(may_open(PaneId::Operator, &goal()).is_ok());
        let why = may_open(PaneId::Worker(2), &goal()).unwrap_err();
        assert!(why.contains("orch and the operator"), "{why}");

        let why = may_open(PaneId::Orch, &task(Some(PaneId::Worker(2)), None)).unwrap_err();
        assert!(why.contains("--parent"), "{why}");
        assert!(may_open(PaneId::Orch, &task(Some(PaneId::Worker(2)), Some(1))).is_ok());
        assert!(may_open(PaneId::Operator, &task(None, None)).is_ok());

        assert!(may_open(PaneId::Worker(2), &task(None, None)).is_ok());
        let why = may_open(PaneId::Worker(2), &task(Some(PaneId::Worker(3)), None)).unwrap_err();
        assert!(why.contains("unowned"), "{why}");
    }

    #[test]
    fn taking_up_sets_the_owner_and_the_chain_keeps_every_entry() {
        let log = vec![
            opened(1, PaneId::Orch, goal()),
            opened(2, PaneId::Orch, task(Some(PaneId::Worker(2)), Some(1))),
            set(2, PaneId::Worker(3), TaskStatus::InProgress),
            set(2, PaneId::Worker(3), TaskStatus::Done),
            set(9, PaneId::Worker(3), TaskStatus::Done),
        ];
        let board = board(&log);
        assert_eq!(board.iter().map(|r| r.number).collect::<Vec<_>>(), vec![1, 2]);
        let record = &board[1];
        assert_eq!(record.creator, PaneId::Orch);
        assert_eq!(record.status, TaskStatus::Done);
        assert_eq!(record.owner.as_ref().unwrap().pane, PaneId::Worker(3));
        assert_eq!(record.chain.len(), 3);
        assert!(matches!(record.chain[1].entry, ChainEntry::TakenUp { .. }));
        assert_eq!(record.chain[1].run, RUN);
    }

    #[test]
    fn done_is_the_owners_in_this_lineage_and_taking_up_is_anyones() {
        let log = vec![
            opened(1, PaneId::Orch, goal()),
            opened(2, PaneId::Orch, task(None, Some(1))),
            set(2, PaneId::Worker(2), TaskStatus::InProgress),
        ];
        let board = board(&log);
        let record = &board[1];
        assert!(record.may_set(PaneId::Worker(2), LINEAGE, TaskStatus::Done).is_ok());
        assert!(record.may_set(PaneId::Worker(3), LINEAGE, TaskStatus::InProgress).is_ok());

        let why = record.may_set(PaneId::Worker(3), LINEAGE, TaskStatus::Done).unwrap_err();
        assert!(why.contains("owner is worker-2") && why.contains("--status in-progress"), "{why}");
        let why = record.may_set(PaneId::Operator, LINEAGE, TaskStatus::Done).unwrap_err();
        assert!(why.contains("only the owner"), "{why}");
        let why = record.may_set(PaneId::Worker(2), "lin-2", TaskStatus::Done).unwrap_err();
        assert!(why.contains("worker-2, earlier run"), "{why}");

        let why = board[0].may_set(PaneId::Orch, LINEAGE, TaskStatus::Done).unwrap_err();
        assert!(why.contains("is a goal"), "{why}");
    }

    #[test]
    fn dropping_and_reopening_follow_who_created_the_task() {
        let log = vec![
            opened(1, PaneId::Orch, goal()),
            opened(2, PaneId::Worker(2), task(None, None)),
            opened(3, PaneId::Operator, task(None, None)),
        ];
        let board = board(&log);
        for status in [TaskStatus::Dropped, TaskStatus::Planned] {
            assert!(board[0].may_set(PaneId::Orch, LINEAGE, status).is_ok(), "its creator");
            assert!(board[1].may_set(PaneId::Worker(2), LINEAGE, status).is_ok(), "its creator");
            assert!(board[1].may_set(PaneId::Orch, LINEAGE, status).is_ok(), "orch on a worker's");
            assert!(board[1].may_set(PaneId::Worker(3), LINEAGE, status).is_err(), "a peer");
            assert!(board[2].may_set(PaneId::Orch, LINEAGE, status).is_err(), "orch on the operator's");
            assert!(board[2].may_set(PaneId::Operator, LINEAGE, status).is_ok());
            assert!(board[0].may_set(PaneId::Operator, LINEAGE, status).is_ok(), "the operator on any");
        }
    }

    #[test]
    fn a_comment_changes_neither_status_nor_owner() {
        let log = vec![
            opened(1, PaneId::Orch, goal()),
            opened(2, PaneId::Orch, task(Some(PaneId::Worker(2)), Some(1))),
            ChainEntry::comment(" the tokenizer leaks ").unwrap().into_event(2, PaneId::Worker(3), RUN, LINEAGE),
        ];
        let record = board(&log).remove(1);
        assert_eq!(record.status, TaskStatus::Planned);
        assert_eq!(record.owner.unwrap().pane, PaneId::Worker(2));
        assert_eq!(record.chain[1].entry, ChainEntry::Commented { text: "the tokenizer leaks".into() });
        assert!(ChainEntry::comment("  ").unwrap_err().contains("needs text"));
    }

    #[test]
    fn an_edit_keeps_the_whole_old_and_new_value_and_refuses_a_no_op() {
        let log = vec![opened(1, PaneId::Orch, goal()), opened(2, PaneId::Orch, task(None, Some(1)))];
        let record = board(&log).remove(1);

        let entries = record
            .edits(PaneId::Orch, Some(&record.block.outcome), &crits(&["cargo test -p parser", "clippy is clean"]), &[])
            .unwrap();
        assert_eq!(
            entries,
            vec![ChainEntry::Edited {
                field: Field::Technical,
                old: crits(&["cargo test -p parser"]),
                new: crits(&["cargo test -p parser", "clippy is clean"]),
            }],
            "the unchanged outcome produced no entry",
        );
        let edited = [log, vec![entries[0].clone().into_event(2, PaneId::Orch, RUN, LINEAGE)]].concat();
        let after = board(&edited).remove(1);
        assert_eq!(after.block.technical.len(), 2);
        assert_eq!(after.chain.len(), 2);

        let why = record.edits(PaneId::Orch, Some("the parser accepts nested groups"), &[], &[]).unwrap_err();
        assert!(why.contains("changes nothing"), "{why}");
        let why = record.edits(PaneId::Orch, None, &[], &[]).unwrap_err();
        assert!(why.contains("changes nothing"), "{why}");
        let why = record.edits(PaneId::Orch, None, &crits(&[" "]), &[]).unwrap_err();
        assert!(why.contains("--crit-t cannot be replaced with nothing"), "{why}");
        let why = record.edits(PaneId::Worker(2), Some("easier"), &[], &[]).unwrap_err();
        assert!(why.contains("opened by orch") && why.contains("fleet task comment 2"), "{why}");
    }

    #[test]
    fn a_chain_event_round_trips_through_json() {
        for event in [
            opened(1, PaneId::Orch, goal()),
            opened(2, PaneId::Orch, task(Some(PaneId::Worker(2)), Some(1))),
            set(2, PaneId::Worker(2), TaskStatus::InProgress),
            ChainEntry::comment("found a leak").unwrap().into_event(2, PaneId::Worker(3), RUN, LINEAGE),
            ChainEntry::Edited { field: Field::Outcome, old: crits(&["a"]), new: crits(&["b"]) }
                .into_event(2, PaneId::Orch, RUN, LINEAGE),
            ChainEntry::status(TaskStatus::Done, Some("cargo test passes")).into_event(2, PaneId::Worker(2), RUN, LINEAGE),
        ] {
            let line = serde_json::to_string(&event).unwrap();
            assert!(line.contains(r#""type":"chain""#), "{line}");
            assert_eq!(serde_json::from_str::<FleetEvent>(&line).unwrap(), event, "{line}");
        }
        let record = board(&[opened(1, PaneId::Orch, goal())]).remove(0);
        let line = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<TaskRecord>(&line).unwrap(), record, "{line}");
    }
}
