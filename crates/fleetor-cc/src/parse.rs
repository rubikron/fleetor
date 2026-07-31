//! Line-oriented NDJSON parser. One JSON object per line; blank lines skipped.

use crate::event::*;
use serde_json::Value;

/// Parse a single NDJSON line into a typed [`Event`]. Returns `None` for blank
/// lines; malformed JSON is surfaced as `Err` so callers can log-and-continue
/// rather than aborting a stream on one bad line.
pub fn parse_line(line: &str) -> Option<anyhow::Result<Event>> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(parse_value(line))
}

fn parse_value(line: &str) -> anyhow::Result<Event> {
    let v: Value = serde_json::from_str(line)?;
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");

    Ok(match (ty, subtype) {
        ("system", "init") => Event::Init(serde_json::from_value(v)?),
        ("assistant", _) => Event::Assistant(serde_json::from_value(v)?),
        ("user", _) => Event::User(serde_json::from_value(v)?),
        ("result", _) => Event::Result(serde_json::from_value(v)?),
        _ => Event::Other(v),
    })
}

/// Parse a whole transcript, dropping blank lines and skipping (not failing on)
/// unparseable lines. Convenience for fixtures and post-hoc analysis.
pub fn parse_transcript(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(parse_line)
        .filter_map(Result::ok)
        .collect()
}
