//! Pane identity and lifecycle (D-030, `docs/tui-pivot-plan.md`).
//!
//! In the TUI fleet every agent is a live `claude` terminal, so identity is
//! **which pane you are**, not which role you play — it replaced a `Party` enum
//! that named roles (lead, worker slot), deleted in Phase 5.
//!
//! [`PaneId`] serializes as a
//! bare string (`"orch"`, `"worker-2"`) precisely so the CLI argument, the DB
//! payload, the event field and the TypeScript type are all the same thing —
//! there is no second spelling to keep in sync.
//!

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// The worker slots a default fleet runs. The orchestrator is not a slot.
pub const WORKER_SLOTS: [u8; 4] = [1, 2, 3, 4];

/// One terminal in the fleet. `Orch` is the operator's own orchestrator TUI;
/// `Worker(n)` is worker pane `n`.
///
/// Ordering is declaration order — orch sorts before every worker — which is the
/// order the roster and the UI band want.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PaneId {
    Orch,
    Worker(u8),
}

impl PaneId {
    /// The worker slot number, or `None` for the orchestrator.
    pub fn slot(self) -> Option<u8> {
        match self {
            PaneId::Orch => None,
            PaneId::Worker(n) => Some(n),
        }
    }

    pub fn is_orch(self) -> bool {
        matches!(self, PaneId::Orch)
    }

    /// The full roster for a fleet with these worker slots: orch first, then the
    /// workers in the order given.
    pub fn roster(workers: &[u8]) -> Vec<PaneId> {
        std::iter::once(PaneId::Orch).chain(workers.iter().copied().map(PaneId::Worker)).collect()
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaneId::Orch => f.write_str("orch"),
            PaneId::Worker(n) => write!(f, "worker-{n}"),
        }
    }
}

/// A pane name that isn't one. Carries the offending input so the `fleet` CLI can
/// say what it was handed — the model reads its own Bash stderr and self-corrects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePaneIdError(pub String);

impl fmt::Display for ParsePaneIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a pane — expected \"orch\" or a worker like \"2\" / \"worker-2\"", self.0)
    }
}

impl std::error::Error for ParsePaneIdError {}

impl FromStr for PaneId {
    type Err = ParsePaneIdError;

    /// Deliberately generous, because a model types this by hand: `orch`, `lead`,
    /// `2`, `w2`, `worker2` and `worker-2` all parse. Whether the slot actually
    /// exists is the hub's business, not the parser's — it is the hub that holds
    /// the roster.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim().to_ascii_lowercase();
        if matches!(t.as_str(), "orch" | "orchestrator" | "lead" | "o") {
            return Ok(PaneId::Orch);
        }
        let digits = t
            .strip_prefix("worker-")
            .or_else(|| t.strip_prefix("worker"))
            .or_else(|| t.strip_prefix('w'))
            .unwrap_or(&t);
        digits.parse::<u8>().map(PaneId::Worker).map_err(|_| ParsePaneIdError(s.to_string()))
    }
}

impl Serialize for PaneId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PaneId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(de::Error::custom)
    }
}

/// Where a pane is in its life. There is no `Idle`/`Working` distinction: a live
/// `claude` TUI is always writable, which is the whole reason the mail queue and
/// the turn-boundary machinery go away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneState {
    /// Process spawned, TUI not yet at a prompt. Not writable.
    Spawning,
    /// At a prompt and accepting input.
    Live,
    /// Process gone.
    Dead,
}

impl PaneState {
    /// Reached a prompt. A **display** predicate — the roster and the band use it.
    /// Do not gate delivery on it; see [`PaneState::accepts_input`].
    pub fn is_live(self) -> bool {
        matches!(self, PaneState::Live)
    }

    /// **The predicate the delivery path must use.** A pane accepts input unless
    /// its process is gone.
    ///
    /// Deliberately *not* `is_live()`. Nothing tells us when `claude` reaches its
    /// prompt — `Spawning` is a guess about a running process, and gating sends on
    /// it means a healthy pane silently refuses every message because our guess
    /// has not flipped yet. That is the exact shape of L1: it looks like it works
    /// and it doesn't. A write to a still-booting pty is buffered by the kernel
    /// and read when the TUI starts reading, which is the failure we can live
    /// with; refusing a live pane is not.
    pub fn accepts_input(self) -> bool {
        !matches!(self, PaneState::Dead)
    }
}

/// One row of the roster: a pane and what it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneEntry {
    pub pane: PaneId,
    pub state: PaneState,
}

impl PaneEntry {
    pub fn new(pane: PaneId, state: PaneState) -> Self {
        Self { pane, state }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_as_the_bare_string_the_cli_and_ui_share() {
        assert_eq!(PaneId::Orch.to_string(), "orch");
        assert_eq!(PaneId::Worker(2).to_string(), "worker-2");
    }

    #[test]
    fn parses_every_spelling_a_model_is_likely_to_type() {
        for s in ["orch", "Orch", " orch ", "lead", "o", "ORCHESTRATOR"] {
            assert_eq!(s.parse::<PaneId>().unwrap(), PaneId::Orch, "{s}");
        }
        for s in ["2", "w2", "W2", "worker2", "worker-2", " worker-2 "] {
            assert_eq!(s.parse::<PaneId>().unwrap(), PaneId::Worker(2), "{s}");
        }
    }

    #[test]
    fn rejects_a_non_pane_and_names_what_it_was_given() {
        let err = "sidebar".parse::<PaneId>().unwrap_err();
        assert_eq!(err.0, "sidebar");
        assert!(err.to_string().contains("sidebar"), "the error quotes the input");
    }

    #[test]
    fn round_trips_through_json_as_a_bare_string() {
        for pane in [PaneId::Orch, PaneId::Worker(4)] {
            let json = serde_json::to_string(&pane).unwrap();
            assert_eq!(json, format!("\"{pane}\""), "serialized form is the display form");
            assert_eq!(serde_json::from_str::<PaneId>(&json).unwrap(), pane);
        }
    }

    #[test]
    fn roster_puts_orch_first() {
        assert_eq!(
            PaneId::roster(&WORKER_SLOTS),
            vec![
                PaneId::Orch,
                PaneId::Worker(1),
                PaneId::Worker(2),
                PaneId::Worker(3),
                PaneId::Worker(4)
            ]
        );
    }

    #[test]
    fn is_live_means_reached_a_prompt() {
        assert!(PaneState::Live.is_live());
        assert!(!PaneState::Spawning.is_live());
        assert!(!PaneState::Dead.is_live());
    }

    /// Only a dead pane refuses input. A still-spawning one must not, or a
    /// healthy pane silently drops every message until our guess catches up.
    #[test]
    fn only_a_dead_pane_refuses_input() {
        assert!(PaneState::Live.accepts_input());
        assert!(PaneState::Spawning.accepts_input());
        assert!(!PaneState::Dead.accepts_input());
    }
}
