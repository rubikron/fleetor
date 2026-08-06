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

/// One participant in the fleet. `Orch` is the operator's own orchestrator TUI;
/// `Worker(n)` is worker pane `n`; `Operator` is the human, who has no terminal
/// of their own (WP-07).
///
/// Ordering is declaration order — the operator first, then orch, then every
/// worker — which is the order the roster wants.
///
/// **`Operator` is the one variant with no pty behind it**, and every asymmetry
/// in this package falls out of that single fact rather than out of a flag:
/// nothing spawns it, nothing kills it, and a message addressed to it is
/// `recorded` rather than `accepted`. See [`PaneId::has_pty`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PaneId {
    /// The human at the keyboard. A name in the record, never a terminal.
    Operator,
    Orch,
    Worker(u8),
}

impl PaneId {
    /// The worker slot number, or `None` for the orchestrator and the operator.
    pub fn slot(self) -> Option<u8> {
        match self {
            PaneId::Operator | PaneId::Orch => None,
            PaneId::Worker(n) => Some(n),
        }
    }

    pub fn is_orch(self) -> bool {
        matches!(self, PaneId::Orch)
    }

    pub fn is_operator(self) -> bool {
        matches!(self, PaneId::Operator)
    }

    /// Whether there is a terminal behind this name.
    ///
    /// False for exactly one identity, and that is the whole of WP-07's
    /// asymmetry. The operator is a participant in the record, not a pane:
    /// there is nothing to type into, so a message addressed to them is
    /// **`recorded`** — it entered the log — and never `accepted`, which means
    /// bytes reached a live pty and nothing else (Tier 1.5, L3). A pty that
    /// does not exist cannot have received any.
    ///
    /// It is also why the operator is never spawnable or killable: the spawn
    /// path has no command to run for a name with no process.
    pub fn has_pty(self) -> bool {
        !matches!(self, PaneId::Operator)
    }

    /// The full roster of **panes** for a fleet with these worker slots: orch
    /// first, then the workers in the order given.
    ///
    /// The operator is deliberately not in it. This list is what spawns, what a
    /// broadcast fans out to, and whose names a brief's peer list is built from
    /// — three things the human is not. The one place the operator joins a
    /// roster is the `fleet roster` *listing*, which the hub assembles.
    pub fn roster(workers: &[u8]) -> Vec<PaneId> {
        std::iter::once(PaneId::Orch).chain(workers.iter().copied().map(PaneId::Worker)).collect()
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaneId::Operator => f.write_str("operator"),
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
        write!(
            f,
            "{:?} is not a fleet name — expected \"orch\", \"operator\", \
             or a worker like \"2\" / \"worker-2\"",
            self.0
        )
    }
}

impl std::error::Error for ParsePaneIdError {}

impl FromStr for PaneId {
    type Err = ParsePaneIdError;

    /// Deliberately generous, because a model types this by hand: `orch`, `lead`,
    /// `2`, `w2`, `worker2` and `worker-2` all parse. Whether the slot actually
    /// exists is the hub's business, not the parser's — it is the hub that holds
    /// the roster.
    ///
    /// **`operator` is an exact match with no aliases (WP-07)**, and `o` keeps
    /// meaning `orch`. The generosity above exists because a model types a
    /// *pane* name constantly; the operator is addressed rarely and by one
    /// name that both briefs spell out, so a short alias would only buy a way
    /// to reach the human by accident.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim().to_ascii_lowercase();
        if matches!(t.as_str(), "orch" | "orchestrator" | "lead" | "o") {
            return Ok(PaneId::Orch);
        }
        if t == "operator" {
            return Ok(PaneId::Operator);
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
    /// **The operator's state, and only ever the operator's** (WP-07). There is
    /// no process, so none of the three words above can be true of them: they
    /// are not spawning, they are not at a prompt, and they have not exited.
    ///
    /// A separate word rather than a borrowed `Live` on purpose. `Live` is a
    /// claim about a terminal — the roster's dot, the delivery predicate and
    /// the operator's own reading of "is that pane up" all rest on it — and a
    /// participant with no terminal wearing it would be the same class of lie
    /// as rendering `accepted` for a message no pty received.
    Present,
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
    ///
    /// [`PaneState::Present`] is false here for a different reason than `Dead`
    /// is: not "the process has gone" but "there was never a process." Nothing
    /// asks — the operator's messages never enter the delivery path at all —
    /// but a `true` here would be a standing invitation for something to try
    /// writing to a pty that does not exist.
    pub fn accepts_input(self) -> bool {
        matches!(self, PaneState::Spawning | PaneState::Live)
    }
}

/// A read-only estimate of how much of a pane's context window is in use,
/// sampled from that pane's own Claude Code transcript (WP-04,
/// `docs/context-gauge-notes.md`).
///
/// Never fabricated. A pane nobody has sampled yet — or the orchestrator,
/// which is deliberately never sampled at all (its transcript is the
/// operator's own, private) — simply has no `ContextGauge`, never a zero or a
/// guess dressed as a measurement (`decisions.md` L155: "band metrics we
/// don't yet track are omitted, not faked").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGauge {
    /// The transcript's own usage fields, summed — see the notes doc for why
    /// this must be `input + cache_creation_input + cache_read_input` and not
    /// `input_tokens` alone (prompt caching makes that one drop the instant it
    /// stops being the honest number).
    pub used_tokens: u32,
    /// The model's real context window — a Tier 2 constant per pane class
    /// (`decisions.md`), **not** whatever Claude Code silently assumes for a
    /// model name it does not recognize.
    pub window_tokens: u32,
    /// `used_tokens / window_tokens` as a whole percent, pre-divided so every
    /// renderer (the `fleet` CLI, the UI band) agrees rather than rounding
    /// differently in Rust and TypeScript.
    pub pct: u8,
}

impl ContextGauge {
    pub fn new(used_tokens: u32, window_tokens: u32) -> Self {
        let pct = if window_tokens == 0 {
            0
        } else {
            ((used_tokens as u64 * 100) / window_tokens as u64).min(100) as u8
        };
        Self { used_tokens, window_tokens, pct }
    }
}

/// One row of the roster: a pane and what it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneEntry {
    pub pane: PaneId,
    pub state: PaneState,
    /// Absent unless something has actually sampled this pane's transcript —
    /// see [`ContextGauge`]. `skip_serializing_if` so an unmeasured pane's
    /// JSON simply omits the key rather than sending `"context":null`; the
    /// frontend already renders a missing field as nothing (`building.md` §4.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextGauge>,
}

impl PaneEntry {
    pub fn new(pane: PaneId, state: PaneState) -> Self {
        Self { pane, state, context: None }
    }

    /// Attach a sampled gauge. A separate builder rather than a `new` arg,
    /// so every existing call site — none of which have a gauge to hand —
    /// stays untouched.
    pub fn with_context(mut self, context: ContextGauge) -> Self {
        self.context = Some(context);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_as_the_bare_string_the_cli_and_ui_share() {
        assert_eq!(PaneId::Orch.to_string(), "orch");
        assert_eq!(PaneId::Worker(2).to_string(), "worker-2");
        assert_eq!(PaneId::Operator.to_string(), "operator");
    }

    #[test]
    fn parses_every_spelling_a_model_is_likely_to_type() {
        for s in ["orch", "Orch", " orch ", "lead", "o", "ORCHESTRATOR"] {
            assert_eq!(s.parse::<PaneId>().unwrap(), PaneId::Orch, "{s}");
        }
        for s in ["2", "w2", "W2", "worker2", "worker-2", " worker-2 "] {
            assert_eq!(s.parse::<PaneId>().unwrap(), PaneId::Worker(2), "{s}");
        }
        // WP-07. Exact, case- and space-insensitive like every other name —
        // and *only* exact: the human gets no short alias.
        for s in ["operator", "Operator", " OPERATOR "] {
            assert_eq!(s.parse::<PaneId>().unwrap(), PaneId::Operator, "{s}");
        }
    }

    /// The alias that was already taken. `o` meant the orchestrator before the
    /// operator existed and must go on meaning it — a model that typed `fleet
    /// send o "…"` expecting orch and reached the human instead would have
    /// escalated to a person by typo.
    #[test]
    fn the_short_o_still_means_orch_and_not_the_operator() {
        assert_eq!("o".parse::<PaneId>().unwrap(), PaneId::Orch);
        assert_eq!("O".parse::<PaneId>().unwrap(), PaneId::Orch);
        for near_miss in ["op", "oper", "human", "operator-1", "operators"] {
            assert!(near_miss.parse::<PaneId>().is_err(), "{near_miss} must not parse");
        }
    }

    /// The one asymmetry the whole package rests on, stated as a predicate so
    /// nothing has to re-derive it from a variant name.
    #[test]
    fn every_name_but_the_operator_has_a_terminal_behind_it() {
        assert!(PaneId::Orch.has_pty());
        assert!(PaneId::Worker(3).has_pty());
        assert!(!PaneId::Operator.has_pty(), "the human is a name in the record, not a pane");
        assert!(PaneId::Operator.is_operator());
        assert!(!PaneId::Operator.is_orch());
        assert_eq!(PaneId::Operator.slot(), None);
    }

    /// The operator is not on the spawn/broadcast roster. Adding it there would
    /// fan every `fleet broadcast` at a pty that does not exist and try to
    /// launch a `claude` for a human.
    #[test]
    fn the_pane_roster_never_contains_the_operator() {
        assert!(!PaneId::roster(&WORKER_SLOTS).contains(&PaneId::Operator));
    }

    #[test]
    fn rejects_a_non_pane_and_names_what_it_was_given() {
        let err = "sidebar".parse::<PaneId>().unwrap_err();
        assert_eq!(err.0, "sidebar");
        assert!(err.to_string().contains("sidebar"), "the error quotes the input");
    }

    #[test]
    fn round_trips_through_json_as_a_bare_string() {
        for pane in [PaneId::Orch, PaneId::Worker(4), PaneId::Operator] {
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
        assert!(!PaneState::Present.is_live(), "the operator must never render as a live pane");
    }

    /// Only a dead pane refuses input. A still-spawning one must not, or a
    /// healthy pane silently drops every message until our guess catches up.
    #[test]
    fn only_a_dead_pane_refuses_input() {
        assert!(PaneState::Live.accepts_input());
        assert!(PaneState::Spawning.accepts_input());
        assert!(!PaneState::Dead.accepts_input());
        assert!(!PaneState::Present.accepts_input(), "there is no pty to write to");
    }

    /// `present` is the operator's word on the roster, and it must be its own
    /// word on the wire too — the frontend renders the state as a label, and a
    /// state that serialized as `live` would put the lie in the JSON itself.
    #[test]
    fn the_operators_state_is_its_own_word_end_to_end() {
        let entry = PaneEntry::new(PaneId::Operator, PaneState::Present);
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["pane"], serde_json::json!("operator"));
        assert_eq!(json["state"], serde_json::json!("present"));
        assert_eq!(serde_json::from_str::<PaneEntry>(&json.to_string()).unwrap(), entry);
    }

    // --- the context gauge (WP-04) ---------------------------------------------

    #[test]
    fn context_gauge_computes_a_whole_percent_of_its_window() {
        assert_eq!(ContextGauge::new(64_000, 128_000).pct, 50);
        assert_eq!(ContextGauge::new(0, 128_000).pct, 0);
        assert_eq!(ContextGauge::new(128_000, 128_000).pct, 100);
    }

    /// A pane that has somehow used more than its assumed window (a wrong
    /// constant, a model that turned out bigger) must not render a percent
    /// over 100 — that reads as a bug in the gauge, not information.
    #[test]
    fn context_gauge_never_reports_over_a_hundred_percent() {
        assert_eq!(ContextGauge::new(500_000, 128_000).pct, 100);
    }

    /// A window of zero is a misconfiguration, not a divide-by-zero panic —
    /// this is read straight into a UI, which must never crash on a bad
    /// constant.
    #[test]
    fn context_gauge_survives_a_zero_window_without_panicking() {
        assert_eq!(ContextGauge::new(100, 0).pct, 0);
    }

    /// A pane nobody has sampled — every pane, at spawn, and the orchestrator
    /// forever — must serialize with the key entirely absent, not `null`,
    /// so the frontend's "missing means nothing" convention applies without
    /// a special case for this field.
    #[test]
    fn an_unsampled_pane_entry_omits_the_context_key_entirely() {
        let entry = PaneEntry::new(PaneId::Worker(2), PaneState::Live);
        let json = serde_json::to_value(&entry).unwrap();
        assert!(!json.as_object().unwrap().contains_key("context"), "{json}");
        assert_eq!(serde_json::from_str::<PaneEntry>(&json.to_string()).unwrap(), entry);
    }

    /// A sampled pane round-trips its gauge, nested under `context`.
    #[test]
    fn a_sampled_pane_entry_carries_its_gauge_through_json() {
        let entry = PaneEntry::new(PaneId::Worker(3), PaneState::Live)
            .with_context(ContextGauge::new(9_000, 128_000));
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["context"]["used_tokens"], serde_json::json!(9_000));
        assert_eq!(json["context"]["pct"], serde_json::json!(7));
        assert_eq!(serde_json::from_str::<PaneEntry>(&json.to_string()).unwrap(), entry);
    }
}
