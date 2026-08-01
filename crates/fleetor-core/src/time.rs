//! Wall-clock helpers. One place so timestamps are consistent everywhere.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch. Saturates to 0 if the clock is before
/// the epoch (never, in practice) rather than panicking.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
