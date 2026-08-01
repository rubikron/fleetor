#!/usr/bin/env node
// fake-claude — a scripted stand-in for the real `claude` CLI (BUILDING §5).
//
// Speaks the same stream-json wire protocol: emits a system/init event on
// startup, then reads NDJSON user messages on stdin and, per the scenario in
// $FAKE_CLAUDE_SCENARIO, streams assistant/result events back on stdout. It
// never calls a model — every supervision test runs against this: fast, free,
// deterministic. Extend it whenever a new failure mode is found in the wild.
//
// Scenarios:
//   happy      init → tool call → final message with a fleet-report(done) → result
//   no-report  turn 1: text + result, no report; turn 2 (reprompt): report → result
//   bad-report a fleet-report block with malformed JSON → result
//   hang       consume the assignment, then never emit a result (tests the watchdog)

import { createInterface } from "node:readline";

const scenario = process.env.FAKE_CLAUDE_SCENARIO || "happy";
const sessionId = `fake-${scenario}-0001`;

function emit(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function assistantText(text) {
  emit({ type: "assistant", message: { role: "assistant", content: [{ type: "text", text }] } });
}

function assistantToolUse(id, name, input) {
  emit({ type: "assistant", message: { role: "assistant", content: [{ type: "tool_use", id, name, input }] } });
}

function toolResult(id, text, isError = false) {
  emit({
    type: "user",
    message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, is_error: isError, content: text }] },
  });
}

function result(subtype = "success", isError = false) {
  emit({
    type: "result",
    subtype,
    is_error: isError,
    num_turns: 1,
    duration_ms: 5,
    session_id: sessionId,
    usage: { input_tokens: 120, output_tokens: 40, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
  });
}

function reportBlock(obj) {
  return "All set.\n\n```fleet-report\n" + JSON.stringify(obj) + "\n```";
}

function ticketIdFrom(userMsg) {
  const text = userMsg?.message?.content?.map((b) => b.text || "").join(" ") || "";
  const m = text.match(/Ticket\s+([A-Za-z0-9-]+)/);
  return m ? m[1] : "T-unknown";
}

// init first, like real CC in streaming-input mode.
emit({
  type: "system",
  subtype: "init",
  session_id: sessionId,
  model: "deepseek-v4-flash",
  cwd: process.cwd(),
  permissionMode: "acceptEdits",
  tools: ["Read", "Write", "Edit", "Bash", "Grep", "Glob"],
  apiKeySource: "none",
});

let turn = 0;
const rl = createInterface({ input: process.stdin });

rl.on("line", (line) => {
  line = line.trim();
  if (!line) return;
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return; // ignore non-JSON stdin
  }
  if (msg.type !== "user") return;
  turn += 1;
  const ticket = ticketIdFrom(msg);

  if (scenario === "hang") {
    // Consume the assignment, acknowledge nothing, never end the turn.
    return;
  }

  if (scenario === "bad-report") {
    assistantText("Here you go:\n\n```fleet-report\n{ this is : not valid json }\n```");
    result();
    return;
  }

  if (scenario === "no-report") {
    if (turn === 1) {
      assistantText("I think I finished but I forgot to file a report.");
      result();
    } else {
      assistantText(reportBlock({ ticket, status: "done", summary: "filed after reprompt", decisions: [], questions: [], risks: [], followups: [] }));
      result();
      rl.close();
    }
    return;
  }

  // happy (default)
  assistantToolUse("tu-1", "Bash", { command: "echo building" });
  toolResult("tu-1", "building\n");
  assistantToolUse("tu-2", "Edit", { file_path: "src/lib.rs", old_string: "a", new_string: "b" });
  toolResult("tu-2", "edited");
  assistantText(reportBlock({
    ticket,
    status: "done",
    summary: "implemented the thing and it builds",
    branch: `ticket/${ticket}`,
    diffstat: "1 file changed, 2 insertions(+)",
    decisions: ["named the helper foo"],
    questions: [],
    risks: [],
    followups: ["consider caching later"],
  }));
  result();
  rl.close();
});

rl.on("close", () => process.exit(0));
