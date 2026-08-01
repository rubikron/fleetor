//! The message envelope (handoff §4, BUILDING §4.1). Versioned from day one so
//! a `v:1` → `v:2` migration costs nothing later. Defined now; the messaging
//! mechanics that fill it (`dm`, `ask_lead`, mail delivery) arrive in Phase 2.

use serde::{Deserialize, Serialize};

pub const ENVELOPE_VERSION: u32 = 1;

/// Who a message is from or to. The lead is the orchestrator; workers are slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Party {
    Lead,
    Worker(u8),
    /// The human at the terminal.
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Worker→worker or lead→worker direct message.
    Dm,
    /// Fire-and-forget progress / broadcast.
    Fyi,
    /// A reply that unblocks an `ask`.
    Answer,
    /// A blocking question (worker→lead).
    Ask,
}

/// Optional pointer to what a message is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Ref {
    Ticket(String),
    Branch(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub from: Party,
    pub to: Party,
    pub kind: MessageKind,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<Ref>,
    pub ts: i64,
    pub v: u32,
}

impl Envelope {
    pub fn new(from: Party, to: Party, kind: MessageKind, body: impl Into<String>) -> Self {
        Self {
            id: crate::ids::new_id("msg"),
            from,
            to,
            kind,
            body: body.into(),
            r#ref: None,
            ts: crate::time::now_ms(),
            v: ENVELOPE_VERSION,
        }
    }
}
