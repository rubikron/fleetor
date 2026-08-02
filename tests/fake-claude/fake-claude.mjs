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
//
// Phase 3 quality-loop scenarios (BUILDING §6 exit test). All self-contained —
// no env, no shared global state — so tests run in parallel safely:
//   qa-bounce  worker: reports done every turn; writes `marker.ok` in cwd only
//              from turn 2 on. So the exit gate `test -f marker.ok` fails on
//              turn 1 and passes after a bounce. Never closes stdin — the
//              supervisor kills the session when the loop ends.
//   qa-clean   worker: like qa-bounce but writes `marker.ok` on turn 1 (gate is
//              green immediately; the only bounce can come from review).
//   review-approve  reviewer: one turn → a fleet-review "approve" block.
//   review-count    reviewer: keeps a counter file in its own cwd; requests
//              changes until the 2nd review, then approves. A fresh reviewer
//              process each round shares the file, driving review→fix→approve.

import { createInterface } from "node:readline";
import net from "node:net";
import fs from "node:fs";
import path from "node:path";

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

function reviewBlock(obj) {
  return "Reviewed.\n\n```fleet-review\n" + JSON.stringify(obj) + "\n```";
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

// --- Phase 2: socket-speaking scenarios (fake worker talks to the fleet hub) ---
// When the scenario starts with "msg-", this fake connects to $FLEET_SOCKET as
// $FLEETOR_SLOT and runs a scripted conversation over the wire, then exits. This
// is how the fake-claude exit test (BUILDING §6) drives a 3-agent conversation
// with a mid-turn delivery without any real tokens.
function fleetConnect() {
  const path = process.env.FLEET_SOCKET;
  const slot = Number(process.env.FLEETOR_SLOT || "0");
  const sock = net.createConnection(path);
  const rl = createInterface({ input: sock });
  const queue = [];
  const waiters = [];
  rl.on("line", (line) => {
    line = line.trim();
    if (!line) return;
    const msg = JSON.parse(line);
    if (waiters.length) waiters.shift()(msg);
    else queue.push(msg);
  });
  const readResp = () =>
    new Promise((resolve) => (queue.length ? resolve(queue.shift()) : waiters.push(resolve)));
  const write = (obj) => sock.write(JSON.stringify(obj) + "\n");
  let reqSeq = 0;
  const call = async (op) => {
    const id = `fake-${slot}-${reqSeq++}`;
    write({ id, ...op });
    return readResp();
  };
  return {
    slot,
    ready: new Promise((res) => sock.on("connect", res)),
    hello: () => write({ party: { kind: "worker", id: slot }, v: 1 }),
    call,
    close: () => sock.end(),
  };
}

async function runSocketScenario() {
  const c = fleetConnect();
  await c.ready;
  c.hello();

  if (scenario === "msg-ask") {
    const resp = await c.call({ op: "ask_lead", question: "Approach A or B?", options: ["A", "B"] });
    assistantText(`lead answered: ${resp.text}`);
    result();
  } else if (scenario === "msg-dm") {
    const to = Number(process.env.FAKE_DM_TO || "3");
    const body = process.env.FAKE_DM_BODY || "note from a peer";
    await c.call({ op: "dm", to, text: body });
    assistantText(`sent dm to worker-${to}`);
    result();
  } else if (scenario === "msg-recv") {
    // Poll the turn-boundary drain until mail arrives (or give up after ~3s).
    let got = [];
    for (let i = 0; i < 60 && got.length === 0; i++) {
      const resp = await c.call({ op: "drain_mail" });
      got = resp.messages || [];
      if (got.length === 0) await new Promise((r) => setTimeout(r, 50));
    }
    assistantText(`received: ${got.map((m) => m.body).join(" | ")}`);
    result();
  }
  c.close();
  process.exit(0);
}

// Phase 4: a stdin-driven worker that also speaks to the hub over the socket.
// Connects, asks the lead a blocking question, then polls the drain for the
// lead's mid-turn mail (the Stop-hook stand-in), and files a `done` report.
async function handleFleetAsk(ticket) {
  const c = fleetConnect();
  await c.ready;
  c.hello();
  const ans = await c.call({ op: "ask_lead", question: `Which approach for ${ticket}?`, options: ["A", "B"] });
  assistantText(`lead answered: ${ans.text}`);

  let mail = [];
  for (let i = 0; i < 40 && mail.length === 0; i++) {
    const resp = await c.call({ op: "drain_mail" });
    mail = resp.messages || [];
    if (mail.length === 0) await new Promise((r) => setTimeout(r, 50));
  }
  if (mail.length) assistantText(`mail received: ${mail.map((m) => m.body).join(" | ")}`);
  c.close();

  assistantText(reportBlock({
    ticket, status: "done", summary: "asked the lead and drained mid-turn mail",
    branch: `ticket/${ticket}`, decisions: [], questions: [], risks: [], followups: [],
  }));
  result();
  // Stay alive; the supervisor kills the session once it ingests the report.
}

// Phase 4b: a worker that files its report over the SOCKET (the `fleet.report`
// MCP tool the hub ingests), NOT as a transcript block. It ends the turn with
// plain text only — so the supervisor's *only* done-signal is the hub's
// `ReportFiled` event (report-over-MCP primary). Proves the transcript scrape is
// no longer required, and that there's exactly one report-filed (no double-log).
async function handleMcpReport(ticket) {
  const c = fleetConnect();
  await c.ready;
  c.hello();
  await c.call({
    op: "report",
    report: {
      ticket, status: "done", summary: "filed over MCP",
      branch: `ticket/${ticket}`, decisions: [], questions: [], risks: [], followups: [],
    },
  });
  c.close();
  assistantText("Done — filed my report via the fleet tool.");
  result();
  // Stay alive; the supervisor kills the session once it reads the hub event.
}

if (scenario.startsWith("msg-")) {
  runSocketScenario();
  // Socket scenarios are self-driving and must NOT set up the stdin loop below:
  // with stdin=null its EOF would fire `close` and exit before the socket work
  // finishes. runSocketScenario() calls process.exit() when done.
} else {
  runStdinLoop();
}

function runStdinLoop() {
let turn = 0;
let assignedTicket = null;
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

  // Phase 4 dual-channel worker: driven over stdin by the supervisor AND talks to
  // the fleet hub over the socket — the two channels a real wired worker uses at
  // once. On assignment it asks the lead a blocking question, then drains the
  // mid-turn mail the lead sends back (standing in for the Stop-hook drain), and
  // finally files a `done` report so the supervisor closes the ticket.
  if (scenario === "fleet-ask") {
    handleFleetAsk(ticket);
    return;
  }

  // Phase 4b worker: file the report over the socket (report-over-MCP primary),
  // ending the turn with no transcript report block. See handleMcpReport above.
  if (scenario === "mcp-report") {
    handleMcpReport(ticket);
    return;
  }

  // Phase 4i standby pool worker: driven purely over stdin (the supervisor delivers
  // each fleet message as a turn). Echo whatever arrives and stay alive for the next
  // one — never close, so the worker idles in the pool between messages.
  if (scenario === "standby") {
    const injected = (msg.message?.content || []).map((b) => b.text || "").join(" ");
    assistantText(`standby-${process.env.FLEETOR_SLOT || "?"} received: ${injected}`);
    result();
    return;
  }

  // D-015 idle→stdin worker: turn 1 ends with NO report (the worker sits idle,
  // waiting for steering). The supervisor's idle-drain then writes queued mail to
  // stdin as a fresh turn; turn 2 sees the framed mail and files a done report.
  if (scenario === "idle-mail") {
    if (turn === 1) {
      assignedTicket = ticket;
      assistantText("Assignment received; standing by for steering before I finalize.");
      result();
    } else {
      const injected = (msg.message?.content || []).map((b) => b.text || "").join(" ");
      assistantText(`idle mail received: ${injected}`);
      assistantText(reportBlock({
        ticket: assignedTicket, status: "done", summary: "acted on idle steering",
        branch: `ticket/${assignedTicket}`, decisions: [], questions: [], risks: [], followups: [],
      }));
      result();
      rl.close();
    }
    return;
  }

  // Phase 3 worker: report done every turn; write the gate marker only once the
  // scenario's turn is reached, so the gate fails first then passes on a bounce.
  if (scenario === "qa-bounce" || scenario === "qa-clean") {
    const markerOnTurn = scenario === "qa-clean" ? 1 : 2;
    if (turn >= markerOnTurn) {
      fs.writeFileSync(path.join(process.cwd(), "marker.ok"), "ok");
    }
    assistantToolUse("tu-1", "Edit", { file_path: "marker.ok", old_string: "a", new_string: "b" });
    toolResult("tu-1", "edited");
    assistantText(reportBlock({ ticket, status: "done", summary: `turn ${turn}`, branch: `ticket/${ticket}`, decisions: [], questions: [], risks: [], followups: [] }));
    result();
    return; // stay alive for further bounces; the supervisor kills the session
  }

  // Phase 3 reviewer: a fresh process per review round, one turn, one verdict.
  if (scenario === "review-approve") {
    assistantText(reviewBlock({ decision: "approve", summary: "meets the AC", blocking: [] }));
    result();
    rl.close();
    return;
  }
  if (scenario === "review-count") {
    const counterPath = path.join(process.cwd(), "review-counter");
    const approveAt = 2;
    let n = 0;
    try { n = Number(fs.readFileSync(counterPath, "utf8")) || 0; } catch { n = 0; }
    n += 1;
    fs.writeFileSync(counterPath, String(n));
    const verdict = n >= approveAt
      ? { decision: "approve", summary: "changes applied", blocking: [] }
      : { decision: "request-changes", summary: "needs a test", blocking: ["add a test for the empty case"] };
    assistantText(reviewBlock(verdict));
    result();
    rl.close();
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
}
