//! The run-log task event as it was before D-100. Nothing writes it any more;
//! it stays decodable so archived runs still digest and replay.

use crate::pane::PaneId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Planned,
    Claimed,
    Done,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBlock {
    pub outcome: String,
    pub technical: Vec<String>,
    pub semantic: Vec<String>,
    pub worker: PaneId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub converges_on: Option<String>,
}

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

#[cfg(test)]
mod tests {
    use crate::event::FleetEvent;

    /// Payloads copied from a pre-D-100 `state.db`: they must decode and
    /// re-serialize unchanged, because a reopen replays them to the UI.
    #[test]
    fn old_task_events_still_round_trip_byte_for_byte() {
        for line in [
            r#"{"type":"task","task":"task-1730413200123-0","from":"orch","at":1730413200123,"change":{"change":"posted","block":{"outcome":"the parser accepts nested groups","technical":["cargo test -p parser"],"semantic":["one grammar"],"worker":"worker-2","parent":"task-1730413200000-9"}}}"#,
            r#"{"type":"task","task":"task-1730413200123-0","from":"worker-2","at":1730413300000,"change":{"change":"updated","status":"claimed","note":"on it"}}"#,
        ] {
            let event: FleetEvent = serde_json::from_str(line).expect("an old task event decodes");
            assert_eq!(event.kind(), "task");
            assert_eq!(serde_json::to_string(&event).unwrap(), line);
        }
    }
}
