//! File-ownership and backlog types (handoff §4 worker-facing tools). Declared
//! ownership is the top concurrency guardrail (handoff §8): two agents in one
//! file is the failure these tools exist to avoid.
//!
//! - `whos_working_on(path)` → [`Owner`]s currently holding a path (a cheap
//!   conflict check before touching shared code).
//! - `claim_file(path)` → a [`LeaseGrant`]: granted, or denied with who holds it.
//! - `backlog_add(text)` → a [`BacklogItem`]: where out-of-scope discoveries go
//!   instead of into the diff.
//!
//! These are pure data; the `leases` and `backlog` tables persist them
//! ([`crate::Store`]), and the hub routes the tools (Phase 3).

use crate::envelope::Party;
use serde::{Deserialize, Serialize};

/// A slot currently holding a path, and the ticket it holds it for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub slot: u8,
    pub ticket: String,
}

/// The answer to a `claim_file`: granted, or denied because a *different* slot
/// already holds the path (handoff §4 "may be denied").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "grant", rename_all = "snake_case")]
pub enum LeaseGrant {
    Granted,
    Denied { held_by: Owner },
}

impl LeaseGrant {
    pub fn is_granted(&self) -> bool {
        matches!(self, LeaseGrant::Granted)
    }
}

/// An out-of-scope discovery parked for later rather than smuggled into the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacklogItem {
    pub id: String,
    pub text: String,
    /// Who raised it (a worker slot, usually).
    pub added_by: Party,
    /// The ticket the discovery surfaced from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    pub ts: i64,
}

impl BacklogItem {
    pub fn new(text: impl Into<String>, added_by: Party, ticket: Option<String>) -> Self {
        Self {
            id: crate::ids::new_id("bk"),
            text: text.into(),
            added_by,
            ticket,
            ts: crate::time::now_ms(),
        }
    }
}
