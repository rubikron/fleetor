//! Process-local unique ids for events and messages.
//!
//! Not cryptographic and not globally unique — a monotonic counter combined
//! with a millisecond timestamp, which is sufficient to distinguish records
//! created within one fleet-server process. Ticket ids come from outside (e.g.
//! `T-041`) and are never generated here.

use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// `<prefix>-<epoch_ms>-<seq>`, e.g. `msg-1730413200123-7`.
pub fn new_id(prefix: &str) -> String {
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{seq}", crate::time::now_ms())
}
