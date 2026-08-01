#!/usr/bin/env bash
# THROWAWAY SPIKE. Drive one isolated Flash `claude -p` turn that must call the
# echo MCP tool, so we can capture CC's MCP handshake + tool namespacing.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
work="$here/work"
config="$work/cc-config"
rm -rf "$work"; mkdir -p "$config"
: > "$here/incoming.log"

# DeepSeek key from the gitignored .env (never hard-coded).
key="$(grep -E '^DEEPSEEK_API_KEY=' "$repo_root/.env" | head -1 | cut -d= -f2- | tr -d '"'"'"' ')"
[ -n "$key" ] || { echo "no DEEPSEEK_API_KEY in $repo_root/.env" >&2; exit 1; }

cat > "$work/mcp.json" <<JSON
{ "mcpServers": { "spike": { "type": "stdio", "command": "node", "args": ["$here/echo-mcp.mjs"] } } }
JSON

echo "=== running claude (Flash, isolated) — must call mcp echo ===" >&2
CLAUDE_CONFIG_DIR="$config" \
ANTHROPIC_BASE_URL="https://api.deepseek.com/anthropic" \
ANTHROPIC_AUTH_TOKEN="$key" \
ANTHROPIC_API_KEY="$key" \
ANTHROPIC_MODEL="deepseek-v4-flash" \
CLAUDE_CODE_EFFORT_LEVEL="max" \
claude -p "Call the echo tool with message 'ping-from-cc' and then tell me exactly what it returned. Do nothing else." \
  --output-format stream-json --verbose \
  --model deepseek-v4-flash \
  --permission-mode acceptEdits \
  --allowedTools "mcp__spike__echo" \
  --mcp-config "$work/mcp.json" \
  > "$here/cc-stream.ndjson" 2>"$here/cc-stderr.log" || {
    echo "claude exited nonzero; see cc-stderr.log" >&2; }

echo "=== tool names seen in CC stream ===" >&2
grep -oE '"name":"[^"]*"' "$here/cc-stream.ndjson" | sort -u >&2 || true
echo "=== incoming.log (MCP server saw) ===" >&2
cat "$here/incoming.log" >&2
