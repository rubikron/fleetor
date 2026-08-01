#!/usr/bin/env bash
# THROWAWAY SPIKE. One isolated Flash `claude -p` turn whose Stop hook injects a
# mid-turn "mail" once. We check: does CC continue the SAME session and act on
# the injected reason (it should output BANANA)?
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
work="$here/work"
config="$work/cc-config"
rm -rf "$work"; mkdir -p "$config"
: > "$here/payloads.log"

key="$(grep -E '^DEEPSEEK_API_KEY=' "$repo_root/.env" | head -1 | cut -d= -f2- | tr -d '"'"'"' ')"
[ -n "$key" ] || { echo "no DEEPSEEK_API_KEY in $repo_root/.env" >&2; exit 1; }

# Stop hook wired via the isolated config dir's settings.json.
cat > "$config/settings.json" <<JSON
{ "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "node $here/stop-hook.mjs" } ] } ] } }
JSON

echo "=== running claude (Flash, isolated) with a one-shot Stop-hook injection ===" >&2
CLAUDE_CONFIG_DIR="$config" \
ANTHROPIC_BASE_URL="https://api.deepseek.com/anthropic" \
ANTHROPIC_AUTH_TOKEN="$key" \
ANTHROPIC_API_KEY="$key" \
ANTHROPIC_MODEL="deepseek-v4-flash" \
CLAUDE_CODE_EFFORT_LEVEL="max" \
claude -p "Say the word HELLO and nothing else." \
  --output-format stream-json --verbose \
  --model deepseek-v4-flash \
  --permission-mode acceptEdits \
  > "$here/cc-stream.ndjson" 2>"$here/cc-stderr.log" || {
    echo "claude exited nonzero; see cc-stderr.log" >&2; }

echo "=== assistant text blocks in order ===" >&2
grep -oE '"text":"[^"]*"' "$here/cc-stream.ndjson" >&2 || true
echo "=== session_id(s) seen (should be ONE — same session continued) ===" >&2
grep -oE '"session_id":"[^"]*"' "$here/cc-stream.ndjson" | sort -u >&2 || true
echo "=== num_turns / result ===" >&2
grep -oE '"num_turns":[0-9]+' "$here/cc-stream.ndjson" >&2 || true
echo "=== payloads.log (what the Stop hook saw) ===" >&2
cat "$here/payloads.log" >&2
