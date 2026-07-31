//! Typed model of Claude Code's `stream-json` (NDJSON) events.
//!
//! Built from real captures in `tests/fixtures/ndjson/`, not from docs — the
//! field names in the handoff are from memory (BUILDING §4). When Claude Code
//! updates, re-capture and diff; this module is the one place that absorbs drift.
//!
//! Parsing is deliberately tolerant: only the events the fleet consumes are
//! modelled, everything else (thinking-token deltas, hook chatter, future
//! subtypes) falls into `Event::Other` rather than failing the line.

use serde::Deserialize;
use serde_json::Value;

/// One decoded NDJSON line.
#[derive(Debug, Clone)]
pub enum Event {
    /// `{"type":"system","subtype":"init", ...}` — session bootstrap.
    Init(InitEvent),
    /// `{"type":"assistant","message":{...}}` — a model turn (text + tool_use).
    Assistant(Message),
    /// `{"type":"user","message":{...}}` — tool results fed back to the model.
    User(Message),
    /// `{"type":"result", ...}` — the turn-end signal that drives supervision.
    Result(ResultEvent),
    /// Anything else: thinking-token deltas, hook events, unknown subtypes.
    Other(Value),
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitEvent {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(rename = "permissionMode", default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(rename = "apiKeySource", default)]
    pub api_key_source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub message: InnerMessage,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InnerMessage {
    #[serde(default)]
    pub role: Option<String>,
    /// `content` may be a string or an array of blocks; we only model the array
    /// form (what CC emits in stream-json) and treat a bare string as one text
    /// block.
    #[serde(default, deserialize_with = "de_content")]
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text(String),
    ToolUse(ToolUse),
    ToolResult(ToolResult),
    /// thinking / redacted_thinking / anything not consumed here.
    Other(Value),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolResult {
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    /// String or array-of-blocks; flatten with [`ToolResult::text`].
    #[serde(default)]
    pub content: Value,
}

impl ToolResult {
    /// Best-effort flatten of `content` to plain text for classification.
    pub fn text(&self) -> String {
        flatten_text(&self.content)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResultEvent {
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// NOTE: computed by Claude Code from its built-in *Anthropic* price table
    /// via model-name mapping — meaningless against DeepSeek. Compute real cost
    /// from `usage` token counts instead. Kept only for reference/telemetry.
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

fn flatten_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|it| it.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn de_content<'de, D>(de: D) -> Result<Vec<ContentBlock>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(de)?;
    Ok(match v {
        Value::String(s) => vec![ContentBlock::Text(s)],
        Value::Array(items) => items.into_iter().map(block_from_value).collect(),
        _ => Vec::new(),
    })
}

fn block_from_value(v: Value) -> ContentBlock {
    match v.get("type").and_then(Value::as_str) {
        Some("text") => ContentBlock::Text(
            v.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        ),
        Some("tool_use") => serde_json::from_value(v.clone())
            .map(ContentBlock::ToolUse)
            .unwrap_or(ContentBlock::Other(v)),
        Some("tool_result") => serde_json::from_value(v.clone())
            .map(ContentBlock::ToolResult)
            .unwrap_or(ContentBlock::Other(v)),
        _ => ContentBlock::Other(v),
    }
}
