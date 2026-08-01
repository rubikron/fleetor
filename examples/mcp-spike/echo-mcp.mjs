#!/usr/bin/env node
// THROWAWAY SPIKE (BUILDING §6 spike-then-commit). Not imported by any crate.
//
// A minimal stdio MCP server: newline-delimited JSON-RPC 2.0. Exposes one tool
// (`echo`). Its whole purpose is to record exactly how the installed `claude`
// discovers and calls an MCP server — the framing, the handshake order, and the
// namespaced tool name CC uses in its stream — so the real fleetor-shim (Phase 2
// step 3) is built from captures, not from the handoff's memory.
//
// Every inbound line is appended to incoming.log; every reply we send is logged
// too. Run indirectly via run-mcp-spike.sh.

import { appendFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const LOG = join(here, "incoming.log");

function log(dir, obj) {
  appendFileSync(LOG, `${dir} ${JSON.stringify(obj)}\n`);
}

function send(obj) {
  log("<<SENT", obj);
  process.stdout.write(JSON.stringify(obj) + "\n");
}

const TOOLS = [
  {
    name: "echo",
    description: "Echo back the message argument. Spike probe tool.",
    inputSchema: {
      type: "object",
      properties: { message: { type: "string", description: "text to echo" } },
      required: ["message"],
    },
  },
];

const rl = createInterface({ input: process.stdin });
rl.on("line", (line) => {
  line = line.trim();
  if (!line) return;
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    log(">>BADLINE", { raw: line });
    return;
  }
  log(">>RECV", msg);

  // Notifications have no id and expect no response.
  if (msg.id === undefined || msg.id === null) return;

  if (msg.method === "initialize") {
    send({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        protocolVersion: msg.params?.protocolVersion || "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "spike", version: "0.0.0" },
      },
    });
  } else if (msg.method === "tools/list") {
    send({ jsonrpc: "2.0", id: msg.id, result: { tools: TOOLS } });
  } else if (msg.method === "tools/call") {
    const args = msg.params?.arguments || {};
    send({
      jsonrpc: "2.0",
      id: msg.id,
      result: { content: [{ type: "text", text: `echo: ${args.message ?? "(none)"}` }] },
    });
  } else if (msg.method === "ping") {
    send({ jsonrpc: "2.0", id: msg.id, result: {} });
  } else {
    send({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: `unknown method ${msg.method}` } });
  }
});
