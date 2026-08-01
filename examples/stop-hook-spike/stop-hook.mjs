#!/usr/bin/env node
// THROWAWAY SPIKE (BUILDING §6). Not imported by any crate.
//
// A `Stop` hook that fires exactly once: on the first turn-end it returns
// {"decision":"block","reason":"<injected mail>"} to make CC continue the same
// session with the reason in context (handoff §5 mid-turn mail delivery); on the
// second turn-end it allows the stop. It records the exact stdin payload CC
// hands a Stop hook to payloads.log, so the real Stop-hook delivery (Phase 2
// step 4) is built from a capture, not from memory.

import { appendFileSync, existsSync, writeFileSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const LOG = join(here, "payloads.log");
const MARKER = join(here, "work", ".fired");

let input = "";
try {
  input = readFileSync(0, "utf8"); // stdin
} catch {}
appendFileSync(LOG, `STDIN ${input.trim()}\n`);

let payload = {};
try {
  payload = JSON.parse(input);
} catch {}

// Loop guard: only inject once. `stop_hook_active` should also be true on the
// re-entry; we log both signals to see which CC actually sets.
appendFileSync(LOG, `SIGNAL stop_hook_active=${payload.stop_hook_active} markerExists=${existsSync(MARKER)}\n`);

if (existsSync(MARKER) || payload.stop_hook_active === true) {
  appendFileSync(LOG, `DECISION allow\n`);
  process.exit(0); // allow the stop
}

writeFileSync(MARKER, "1");
const out = {
  decision: "block",
  reason: "SYSTEM (fleet mail): the lead says reply with exactly the single word BANANA and nothing else.",
};
appendFileSync(LOG, `DECISION ${JSON.stringify(out)}\n`);
process.stdout.write(JSON.stringify(out));
process.exit(0);
